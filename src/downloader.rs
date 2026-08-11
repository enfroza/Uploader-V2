use anyhow::{anyhow, Result};
use serde::Deserialize;
use std::env;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

const TIKWM_API_URL: &str = "https://www.tikwm.com/api/";
const TIKWM_BASE_URL: &str = "https://www.tikwm.com";
const HTTP_TIMEOUT: Duration = Duration::from_secs(90);

#[derive(Debug, Deserialize)]
pub struct TikWmResponse {
    pub code: i32,
    pub msg: Option<String>,
    pub data: Option<TikWmData>,
}

#[derive(Debug, Deserialize)]
pub struct TikWmData {
    pub id: Option<String>,
    pub title: Option<String>,
    pub play: Option<String>,
    pub hdplay: Option<String>,
    pub size: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct VideoInfo {
    pub title: String,
    pub download_url: String,
    pub size_bytes: Option<u64>,
}

pub struct Downloader {
    client: reqwest::Client,
    cookies: Option<String>,
}

fn origin_from_url(url: &str) -> String {
    if let Ok(parsed) = url::Url::parse(url) {
        if let Some(host) = parsed.host_str() {
            return format!("{}://{}/", parsed.scheme(), host);
        }
    }
    url.to_string()
}

impl Downloader {
    pub fn new() -> Self {
        let cookies = env::var("DIRECT_VIDEO_COOKIES")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        if cookies.is_some() {
            log::info!("DIRECT_VIDEO_COOKIES loaded");
        }

        let client = reqwest::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .connect_timeout(Duration::from_secs(20))
            .user_agent(
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
                 AppleWebKit/537.36 (KHTML, like Gecko) \
                 Chrome/122.0.0.0 Safari/537.36",
            )
            .redirect(reqwest::redirect::Policy::limited(10))
            .gzip(true)
            .brotli(true)
            .http1_only()
            .pool_max_idle_per_host(2)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self { client, cookies }
    }

    fn browser_headers(&self, video_url: &str) -> reqwest::header::HeaderMap {
        use reqwest::header::{
            HeaderMap, HeaderValue, ACCEPT, ACCEPT_LANGUAGE, COOKIE, ORIGIN, REFERER,
        };

        let mut headers = HeaderMap::new();
        let origin = origin_from_url(video_url);

        headers.insert(ACCEPT, HeaderValue::from_static("*/*"));
        headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("en-US,en;q=0.9"));

        if let Ok(v) = HeaderValue::from_str(&origin) {
            headers.insert(REFERER, v.clone());
            headers.insert(ORIGIN, v);
        }

        if let Some(ref cookie_str) = self.cookies {
            if let Ok(v) = HeaderValue::from_str(cookie_str) {
                headers.insert(COOKIE, v);
            }
        }

        headers
    }

    pub async fn fetch_video_info(&self, tiktok_url: &str) -> Result<VideoInfo> {
        let params = [("url", tiktok_url), ("hd", "1")];
        let req = self.client.post(TIKWM_API_URL).form(&params).send();

        let response = match timeout(HTTP_TIMEOUT, req).await {
            Ok(res) => res?,
            Err(_) => return Err(anyhow!("Request to TikWM API timed out after 90 seconds")),
        };

        if !response.status().is_success() {
            return Err(anyhow!("TikWM API returned HTTP status {}", response.status()));
        }

        let tikwm_res: TikWmResponse = response.json().await?;

        if tikwm_res.code != 0 {
            let msg = tikwm_res.msg.unwrap_or_else(|| "Unknown API error".to_string());
            return Err(anyhow!("TikWM API error (code {}): {}", tikwm_res.code, msg));
        }

        let data = tikwm_res
            .data
            .ok_or_else(|| anyhow!("TikWM API returned no data payload"))?;

        let raw_url = data
            .hdplay
            .filter(|s| !s.is_empty())
            .or(data.play)
            .ok_or_else(|| anyhow!("No video download URL found in API response"))?;

        let download_url = if raw_url.starts_with('/') {
            format!("{}{}", TIKWM_BASE_URL, raw_url)
        } else {
            raw_url
        };

        Ok(VideoInfo {
            title: data.title.unwrap_or_else(|| "TikTok Video".to_string()),
            download_url,
            size_bytes: data.size,
        })
    }

    pub async fn probe_direct_url(&self, video_url: &str) -> Result<Option<u64>> {
        let headers = self.browser_headers(video_url);
        let req = self.client.head(video_url).headers(headers).send();

        let response = match timeout(HTTP_TIMEOUT, req).await {
            Ok(res) => res?,
            Err(_) => return Ok(None),
        };

        if !response.status().is_success() {
            return Ok(None);
        }

        Ok(response.content_length())
    }

    /// Try normal HTTP first. On failure → fall back to yt-dlp.
    pub async fn download_video_bytes(&self, video_url: &str, max_bytes: u64) -> Result<Vec<u8>> {
        match self.download_via_http(video_url, max_bytes).await {
            Ok(bytes) => return Ok(bytes),
            Err(e) => {
                log::warn!("HTTP download failed ({}), falling back to yt-dlp...", e);
            }
        }

        self.download_via_ytdlp(video_url, max_bytes).await
    }

    async fn download_via_http(&self, video_url: &str, max_bytes: u64) -> Result<Vec<u8>> {
        let headers = self.browser_headers(video_url);
        let req = self.client.get(video_url).headers(headers).send();

        let response = match timeout(HTTP_TIMEOUT, req).await {
            Ok(res) => res?,
            Err(_) => return Err(anyhow!("Video download timed out after 90 seconds")),
        };

        let status = response.status();
        if !status.is_success() {
            return Err(anyhow!("HTTP {}", status));
        }

        if let Some(content_length) = response.content_length() {
            if content_length > max_bytes {
                return Err(anyhow!(
                    "Video size ({:.2} MB) exceeds Telegram limit of 50 MB",
                    content_length as f64 / (1024.0 * 1024.0)
                ));
            }
        }

        let bytes = response.bytes().await?;

        if bytes.len() as u64 > max_bytes {
            return Err(anyhow!(
                "Downloaded video size ({:.2} MB) exceeds Telegram limit of 50 MB",
                bytes.len() as f64 / (1024.0 * 1024.0)
            ));
        }

        if bytes.is_empty() {
            return Err(anyhow!("Downloaded file is empty"));
        }

        Ok(bytes.to_vec())
    }

    async fn download_via_ytdlp(&self, video_url: &str, max_bytes: u64) -> Result<Vec<u8>> {
        let temp_dir = std::env::temp_dir();
        let out_template = temp_dir.join(format!("ytdlp_{}.%(ext)s", uuid::Uuid::new_v4()));
        let out_template_str = out_template.to_string_lossy().to_string();

        log::info!("Starting yt-dlp download for {}", video_url);

        let mut cmd = Command::new("yt-dlp");
        cmd.arg("--no-playlist")
            .arg("--no-warnings")
            .arg("--no-progress")
            .arg("-f")
            .arg("best[ext=mp4]/best[ext=webm]/best")
            .arg("--max-filesize")
            .arg(format!("{}M", max_bytes / (1024 * 1024)))
            .arg("--extractor-args")
            .arg("generic:impersonate")
            .arg("-o")
            .arg(&out_template_str)
            .arg(video_url)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        // Optional cookies file
        if let Ok(cookies_file) = env::var("YTDLP_COOKIES") {
            if !cookies_file.is_empty() {
                cmd.arg("--cookies").arg(cookies_file);
            }
        }

        // Optional extra args from env (can override / add more)
        if let Ok(extra) = env::var("YTDLP_ARGS") {
            for arg in extra.split_whitespace() {
                cmd.arg(arg);
            }
        }

        let output = timeout(Duration::from_secs(180), cmd.output())
            .await
            .map_err(|_| anyhow!("yt-dlp timed out after 180 seconds"))?
            .map_err(|e| anyhow!("Failed to start yt-dlp: {}. Is yt-dlp installed?", e))?;

        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);

        if !output.status.success() {
            return Err(anyhow!(
                "yt-dlp failed: {} {}",
                stdout.trim(),
                stderr.trim()
            ));
        }

        // Find the downloaded file (yt-dlp replaces %(ext)s)
        let parent = out_template.parent().unwrap_or(std::path::Path::new("/tmp"));
        let prefix = out_template
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("ytdlp_");

        let mut downloaded: Option<PathBuf> = None;
        if let Ok(entries) = std::fs::read_dir(parent) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with(prefix) {
                    downloaded = Some(entry.path());
                    break;
                }
            }
        }

        let path = downloaded.ok_or_else(|| {
            anyhow!(
                "yt-dlp finished but output file not found. stderr: {}",
                stderr.trim()
            )
        })?;

        let bytes = tokio::fs::read(&path).await?;
        let _ = tokio::fs::remove_file(&path).await;

        if bytes.len() as u64 > max_bytes {
            return Err(anyhow!(
                "Downloaded video size ({:.2} MB) exceeds Telegram limit of 50 MB",
                bytes.len() as f64 / (1024.0 * 1024.0)
            ));
        }

        if bytes.is_empty() {
            return Err(anyhow!("yt-dlp returned empty file"));
        }

        log::info!("yt-dlp download OK – {} bytes", bytes.len());
        Ok(bytes)
    }
}

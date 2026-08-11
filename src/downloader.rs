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
    /// Path to the Playwright download script
    playwright_script: PathBuf,
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

        // Locate playwright script relative to the binary / project
        let playwright_script = env::var("PLAYWRIGHT_SCRIPT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                // Default: ./playwright-downloader/download.js
                PathBuf::from("playwright-downloader/download.js")
            });

        log::info!("Playwright script: {:?}", playwright_script);

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

        Self {
            client,
            cookies,
            playwright_script,
        }
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

        headers.insert(
            "sec-ch-ua",
            HeaderValue::from_static(
                r#""Chromium";v="122", "Not(A:Brand";v="24", "Google Chrome";v="122""#,
            ),
        );
        headers.insert("sec-ch-ua-mobile", HeaderValue::from_static("?0"));
        headers.insert("sec-ch-ua-platform", HeaderValue::from_static(r#""Windows""#));
        headers.insert("sec-fetch-dest", HeaderValue::from_static("video"));
        headers.insert("sec-fetch-mode", HeaderValue::from_static("no-cors"));
        headers.insert("sec-fetch-site", HeaderValue::from_static("same-origin"));

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
            Err(_) => return Err(anyhow!("HEAD request timed out after 90 seconds")),
        };

        if !response.status().is_success() {
            return Ok(None);
        }

        Ok(response.content_length())
    }

    /// Try normal HTTP download first. On 403 → fall back to Playwright.
    pub async fn download_video_bytes(&self, video_url: &str, max_bytes: u64) -> Result<Vec<u8>> {
        // 1) Try lightweight HTTP first
        match self.download_via_http(video_url, max_bytes).await {
            Ok(bytes) => return Ok(bytes),
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("403") || msg.contains("Forbidden") {
                    log::warn!("HTTP 403 – falling back to Playwright for {}", video_url);
                } else {
                    // Non-403 errors: still try Playwright as last resort
                    log::warn!("HTTP download failed ({}), trying Playwright...", msg);
                }
            }
        }

        // 2) Playwright fallback (handles Cloudflare)
        self.download_via_playwright(video_url, max_bytes).await
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

    async fn download_via_playwright(&self, video_url: &str, max_bytes: u64) -> Result<Vec<u8>> {
        let temp_path = std::env::temp_dir().join(format!(
            "pw_{}.mp4",
            uuid::Uuid::new_v4()
        ));

        let script = &self.playwright_script;
        if !script.exists() {
            return Err(anyhow!(
                "Playwright script not found at {:?}. Run: cd playwright-downloader && npm install && npx playwright install chromium",
                script
            ));
        }

        log::info!("Starting Playwright download → {:?}", temp_path);

        let output = Command::new("node")
            .arg(script)
            .arg(video_url)
            .arg(&temp_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .output()
            .await
            .map_err(|e| anyhow!("Failed to start Playwright: {}. Is Node.js installed?", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if !output.status.success() {
            // Clean up partial file
            let _ = tokio::fs::remove_file(&temp_path).await;
            return Err(anyhow!(
                "Playwright failed: {} {}",
                stdout.trim(),
                stderr.trim()
            ));
        }

        // Parse JSON result
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(stdout.trim()) {
            if json.get("ok").and_then(|v| v.as_bool()) != Some(true) {
                let _ = tokio::fs::remove_file(&temp_path).await;
                let err = json.get("error").and_then(|v| v.as_str()).unwrap_or("unknown");
                return Err(anyhow!("Playwright error: {}", err));
            }
        }

        let bytes = tokio::fs::read(&temp_path).await?;
        let _ = tokio::fs::remove_file(&temp_path).await;

        if bytes.len() as u64 > max_bytes {
            return Err(anyhow!(
                "Downloaded video size ({:.2} MB) exceeds Telegram limit of 50 MB",
                bytes.len() as f64 / (1024.0 * 1024.0)
            ));
        }

        if bytes.is_empty() {
            return Err(anyhow!("Playwright returned empty file"));
        }

        log::info!("Playwright download OK – {} bytes", bytes.len());
        Ok(bytes)
    }
}

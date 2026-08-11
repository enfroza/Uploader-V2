use anyhow::{anyhow, Result};
use serde::Deserialize;
use std::time::Duration;
use tokio::time::timeout;

const TIKWM_API_URL: &str = "https://www.tikwm.com/api/";
const TIKWM_BASE_URL: &str = "https://www.tikwm.com";
const HTTP_TIMEOUT: Duration = Duration::from_secs(60);

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
    pub play: Option<String>,    // Direct video stream URL (no watermark)
    pub hdplay: Option<String>,  // HD video stream URL if available
    pub size: Option<u64>,       // Video size in bytes
}

#[derive(Debug, Clone)]
pub struct VideoInfo {
    pub title: String,
    pub download_url: String,
    pub size_bytes: Option<u64>,
}

pub struct Downloader {
    client: reqwest::Client,
}

impl Downloader {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
            .redirect(reqwest::redirect::Policy::limited(10))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self { client }
    }

    /// Fetches TikTok video metadata via TikWM API
    pub async fn fetch_video_info(&self, tiktok_url: &str) -> Result<VideoInfo> {
        let params = [("url", tiktok_url), ("hd", "1")];

        let req = self.client.post(TIKWM_API_URL).form(&params).send();

        let response = match timeout(HTTP_TIMEOUT, req).await {
            Ok(res) => res?,
            Err(_) => return Err(anyhow!("Request to TikWM API timed out after 60 seconds")),
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

        // Use HD play URL if present, fallback to standard play URL
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

    /// Probes a direct video URL (HEAD request) to get Content-Length when available.
    pub async fn probe_direct_url(&self, video_url: &str) -> Result<Option<u64>> {
        let req = self.client.head(video_url).send();

        let response = match timeout(HTTP_TIMEOUT, req).await {
            Ok(res) => res?,
            Err(_) => return Err(anyhow!("HEAD request timed out after 60 seconds")),
        };

        if !response.status().is_success() {
            // Some hosts reject HEAD; fall back to letting the GET handle it
            return Ok(None);
        }

        Ok(response.content_length())
    }

    /// Downloads the video bytes with a strict size cap to protect RAM/disk.
    /// Works for both TikWM-resolved URLs and arbitrary direct .mp4 links.
    pub async fn download_video_bytes(&self, video_url: &str, max_bytes: u64) -> Result<Vec<u8>> {
        let req = self
            .client
            .get(video_url)
            .header("Accept", "*/*")
            .header("Referer", video_url)
            .send();

        let response = match timeout(HTTP_TIMEOUT, req).await {
            Ok(res) => res?,
            Err(_) => return Err(anyhow!("Video download timed out after 60 seconds")),
        };

        if !response.status().is_success() {
            return Err(anyhow!(
                "Failed to download video stream, HTTP status: {}",
                response.status()
            ));
        }

        // Check Content-Length header if provided by server
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
}

use anyhow::{anyhow, Result};
use serde::Deserialize;
use std::time::Duration;
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
}

/// Extract origin (scheme + host) for a proper Referer/Origin.
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
            // Prefer HTTP/1.1 – some Cloudflare configs treat pure HTTP/2 clients more strictly
            .http1_only()
            .pool_max_idle_per_host(2)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self { client }
    }

    fn browser_headers(video_url: &str) -> reqwest::header::HeaderMap {
        use reqwest::header::{
            HeaderMap, HeaderValue, ACCEPT, ACCEPT_LANGUAGE, ORIGIN, REFERER,
        };

        let mut headers = HeaderMap::new();
        let origin = origin_from_url(video_url);

        headers.insert(ACCEPT, HeaderValue::from_static("*/*"));
        headers.insert(
            ACCEPT_LANGUAGE,
            HeaderValue::from_static("en-US,en;q=0.9"),
        );

        if let Ok(v) = HeaderValue::from_str(&origin) {
            headers.insert(REFERER, v.clone());
            headers.insert(ORIGIN, v);
        }

        // Realistic Chrome client hints
        headers.insert(
            "sec-ch-ua",
            HeaderValue::from_static(
                r#""Chromium";v="122", "Not(A:Brand";v="24", "Google Chrome";v="122""#,
            ),
        );
        headers.insert("sec-ch-ua-mobile", HeaderValue::from_static("?0"));
        headers.insert(
            "sec-ch-ua-platform",
            HeaderValue::from_static(r#""Windows""#),
        );
        headers.insert("sec-fetch-dest", HeaderValue::from_static("video"));
        headers.insert("sec-fetch-mode", HeaderValue::from_static("no-cors"));
        headers.insert("sec-fetch-site", HeaderValue::from_static("same-origin"));
        headers.insert("priority", HeaderValue::from_static("u=1, i"));

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
        let headers = Self::browser_headers(video_url);

        let req = self.client.head(video_url).headers(headers).send();

        let response = match timeout(HTTP_TIMEOUT, req).await {
            Ok(res) => res?,
            Err(_) => return Err(anyhow!("HEAD request timed out after 90 seconds")),
        };

        if !response.status().is_success() {
            log::debug!(
                "HEAD probe for {} returned {}, will try full GET",
                video_url,
                response.status()
            );
            return Ok(None);
        }

        Ok(response.content_length())
    }

    pub async fn download_video_bytes(&self, video_url: &str, max_bytes: u64) -> Result<Vec<u8>> {
        let headers = Self::browser_headers(video_url);

        let req = self.client.get(video_url).headers(headers).send();

        let response = match timeout(HTTP_TIMEOUT, req).await {
            Ok(res) => res?,
            Err(_) => return Err(anyhow!("Video download timed out after 90 seconds")),
        };

        let status = response.status();
        if !status.is_success() {
            return Err(anyhow!(
                "Failed to download video stream, HTTP status: {} \
                 (Cloudflare or the site is blocking this server IP / request fingerprint. \
                 Try from a residential IP or a different VPS.)",
                status
            ));
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
}

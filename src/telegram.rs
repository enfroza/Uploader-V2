use anyhow::{anyhow, Result};
use std::sync::Arc;
use teloxide::prelude::*;
use teloxide::types::InputFile;
use tokio::fs;
use tokio::sync::Semaphore;
use uuid::Uuid;

use crate::downloader::Downloader;

pub const MAX_TELEGRAM_FILE_SIZE: u64 = 50 * 1024 * 1024; // 50 MB limit

/// Returns true if the text looks like a direct video/media URL we can download.
pub fn is_direct_video_url(text: &str) -> bool {
    let lower = text.trim().to_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return false;
    }
    // Common video extensions or known path patterns
    lower.ends_with(".mp4")
        || lower.ends_with(".webm")
        || lower.ends_with(".mkv")
        || lower.ends_with(".mov")
        || lower.ends_with(".m4v")
        || lower.contains(".mp4?")
        || lower.contains("/videos/")
        || lower.contains("/video/")
        || lower.contains("contents/videos")
}

fn extract_filename_from_url(url: &str) -> String {
    url.split('/')
        .last()
        .and_then(|s| {
            let clean = s.split('?').next().unwrap_or(s);
            if clean.is_empty() || !clean.contains('.') {
                None
            } else {
                Some(clean.to_string())
            }
        })
        .unwrap_or_else(|| "video.mp4".to_string())
}

pub async fn handle_tiktok_link(
    bot: Bot,
    msg: Message,
    tiktok_url: String,
    downloader: Arc<Downloader>,
    semaphore: Arc<Semaphore>,
) -> Result<()> {
    // Concurrency Guard: Wait for an available slot (Max 2 concurrent downloads)
    let _permit = match semaphore.acquire().await {
        Ok(permit) => permit,
        Err(_) => return Err(anyhow!("Semaphore closed")),
    };

    let status_msg = bot
        .send_message(msg.chat.id, "⏳ Fetching TikTok video info...")
        .reply_to_message_id(msg.id)
        .await?;

    // 1. Fetch metadata
    let video_info = match downloader.fetch_video_info(&tiktok_url).await {
        Ok(info) => info,
        Err(err) => {
            let _ = bot
                .edit_message_text(msg.chat.id, status_msg.id, format!("❌ Error: {}", err))
                .await;
            return Err(err);
        }
    };

    // Pre-download size check
    if let Some(size) = video_info.size_bytes {
        if size > MAX_TELEGRAM_FILE_SIZE {
            let err_msg = format!(
                "❌ Video size ({:.2} MB) exceeds Telegram's 50 MB upload limit.",
                size as f64 / (1024.0 * 1024.0)
            );
            let _ = bot.edit_message_text(msg.chat.id, status_msg.id, &err_msg).await;
            return Err(anyhow!(err_msg));
        }
    }

    let _ = bot
        .edit_message_text(msg.chat.id, status_msg.id, "📥 Downloading video...")
        .await;

    // 2. Download video bytes to temporary path
    let temp_file_path = std::env::temp_dir().join(format!("tiktok_{}.mp4", Uuid::new_v4()));

    let download_result = async {
        let bytes = downloader
            .download_video_bytes(&video_info.download_url, MAX_TELEGRAM_FILE_SIZE)
            .await?;
        fs::write(&temp_file_path, bytes).await?;
        Ok::<(), anyhow::Error>(())
    }
    .await;

    if let Err(err) = download_result {
        if temp_file_path.exists() {
            let _ = fs::remove_file(&temp_file_path).await;
        }
        let _ = bot
            .edit_message_text(msg.chat.id, status_msg.id, format!("❌ Download failed: {}", err))
            .await;
        return Err(err);
    }

    let _ = bot
        .edit_message_text(msg.chat.id, status_msg.id, "📤 Uploading to Telegram...")
        .await;

    // 3. Send video file to user
    let input_file = InputFile::file(&temp_file_path).file_name("tiktok_video.mp4");
    let caption = if video_info.title.is_empty() {
        "Downloaded via TikTok Bot".to_string()
    } else {
        video_info.title
    };

    let send_result = bot
        .send_video(msg.chat.id, input_file)
        .caption(caption)
        .reply_to_message_id(msg.id)
        .await;

    // 4. Guaranteed Temporary File Cleanup
    if temp_file_path.exists() {
        if let Err(err) = fs::remove_file(&temp_file_path).await {
            log::warn!("Failed to delete temporary file {:?}: {}", temp_file_path, err);
        }
    }

    // Remove status message
    let _ = bot.delete_message(msg.chat.id, status_msg.id).await;

    match send_result {
        Ok(_) => Ok(()),
        Err(err) => {
            let _ = bot
                .send_message(msg.chat.id, format!("❌ Failed to send video: {}", err))
                .await;
            Err(anyhow!(err))
        }
    }
}

/// Handles direct video URLs (e.g. https://jilhub.org/contents/videos/11000/11058/11058.mp4)
pub async fn handle_direct_video_url(
    bot: Bot,
    msg: Message,
    video_url: String,
    downloader: Arc<Downloader>,
    semaphore: Arc<Semaphore>,
) -> Result<()> {
    let _permit = match semaphore.acquire().await {
        Ok(permit) => permit,
        Err(_) => return Err(anyhow!("Semaphore closed")),
    };

    let status_msg = bot
        .send_message(msg.chat.id, "⏳ Checking direct video URL...")
        .reply_to_message_id(msg.id)
        .await?;

    // Optional size probe via HEAD
    if let Ok(Some(size)) = downloader.probe_direct_url(&video_url).await {
        if size > MAX_TELEGRAM_FILE_SIZE {
            let err_msg = format!(
                "❌ Video size ({:.2} MB) exceeds Telegram's 50 MB upload limit.",
                size as f64 / (1024.0 * 1024.0)
            );
            let _ = bot.edit_message_text(msg.chat.id, status_msg.id, &err_msg).await;
            return Err(anyhow!(err_msg));
        }
    }

    let _ = bot
        .edit_message_text(msg.chat.id, status_msg.id, "📥 Downloading video...")
        .await;

    let filename = extract_filename_from_url(&video_url);
    let temp_file_path = std::env::temp_dir().join(format!("direct_{}_{}", Uuid::new_v4(), filename));

    let download_result = async {
        let bytes = downloader
            .download_video_bytes(&video_url, MAX_TELEGRAM_FILE_SIZE)
            .await?;
        fs::write(&temp_file_path, bytes).await?;
        Ok::<(), anyhow::Error>(())
    }
    .await;

    if let Err(err) = download_result {
        if temp_file_path.exists() {
            let _ = fs::remove_file(&temp_file_path).await;
        }
        let _ = bot
            .edit_message_text(msg.chat.id, status_msg.id, format!("❌ Download failed: {}", err))
            .await;
        return Err(err);
    }

    let _ = bot
        .edit_message_text(msg.chat.id, status_msg.id, "📤 Uploading to Telegram...")
        .await;

    let input_file = InputFile::file(&temp_file_path).file_name(filename);
    let caption = "Downloaded via Uploader Bot".to_string();

    let send_result = bot
        .send_video(msg.chat.id, input_file)
        .caption(caption)
        .reply_to_message_id(msg.id)
        .await;

    // Guaranteed cleanup
    if temp_file_path.exists() {
        if let Err(err) = fs::remove_file(&temp_file_path).await {
            log::warn!("Failed to delete temporary file {:?}: {}", temp_file_path, err);
        }
    }

    let _ = bot.delete_message(msg.chat.id, status_msg.id).await;

    match send_result {
        Ok(_) => Ok(()),
        Err(err) => {
            let _ = bot
                .send_message(msg.chat.id, format!("❌ Failed to send video: {}", err))
                .await;
            Err(anyhow!(err))
        }
    }
}


mod downloader;
mod telegram;

use dotenvy::dotenv;
use std::env;
use std::sync::Arc;
use teloxide::prelude::*;
use tokio::sync::Semaphore;

use downloader::Downloader;
use telegram::{handle_direct_video_url, handle_tiktok_link, is_direct_video_url};

#[tokio::main]
async fn main() {
    dotenv().ok();
    env_logger::init();

    log::info!("Starting Uploader-V2 Bot (Rust + low-memory Playwright fallback)...");

    let bot_token = env::var("TELOXIDE_TOKEN")
        .or_else(|_| env::var("TELEGRAM_BOT_TOKEN"))
        .expect("TELEGRAM_BOT_TOKEN or TELOXIDE_TOKEN must be set in environment or .env");

    let bot = Bot::new(bot_token);

    // Limit to 1 concurrent download to keep RAM safe on 4 GB instances
    // (Playwright/Chromium is memory-heavy)
    let semaphore = Arc::new(Semaphore::new(1));
    let downloader = Arc::new(Downloader::new());

    teloxide::repl(bot, move |bot: Bot, msg: Message| {
        let downloader = Arc::clone(&downloader);
        let semaphore = Arc::clone(&semaphore);

        async move {
            if let Some(text) = msg.text().map(|s| s.to_string()) {
                let trimmed = text.trim().to_string();

                if trimmed.contains("tiktok.com") {
                    if let Err(e) =
                        handle_tiktok_link(bot, msg, trimmed, downloader, semaphore).await
                    {
                        log::error!("Error processing TikTok request: {:?}", e);
                    }
                } else if is_direct_video_url(&trimmed) {
                    if let Err(e) =
                        handle_direct_video_url(bot, msg, trimmed, downloader, semaphore).await
                    {
                        log::error!("Error processing direct video URL: {:?}", e);
                    }
                } else if trimmed.starts_with("/start") {
                    let _ = bot
                        .send_message(
                            msg.chat.id,
                            "👋 Send me:\n\
• Any TikTok video link → watermark-free MP4\n\
• Any direct video URL (e.g. `.mp4`) → download & upload\n\n\
Protected sites (Cloudflare) are handled automatically via Playwright.\n\
Only 1 download runs at a time to keep memory low.",
                        )
                        .await;
                }
            }
            Ok(())
        }
    })
    .await;
}

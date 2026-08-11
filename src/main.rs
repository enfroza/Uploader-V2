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

    log::info!("Starting Uploader-V2 Bot (100% Pure Rust)...");

    let bot_token = env::var("TELOXIDE_TOKEN")
        .or_else(|_| env::var("TELEGRAM_BOT_TOKEN"))
        .expect("TELEGRAM_BOT_TOKEN or TELOXIDE_TOKEN must be set in environment or .env");

    let bot = Bot::new(bot_token);

    // Global semaphore capping concurrent video processing to 2 to prevent high RAM usage
    let semaphore = Arc::new(Semaphore::new(2));
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
Example direct link:\n`https://example.com/path/video.mp4`",
                        )
                        .await;
                }
            }
            Ok(())
        }
    })
    .await;
}

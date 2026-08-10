mod downloader;
mod telegram;

use dotenvy::dotenv;
use std::env;
use std::sync::Arc;
use teloxide::prelude::*;
use tokio::sync::Semaphore;

use downloader::Downloader;
use telegram::handle_tiktok_link;

#[tokio::main]
async fn main() {
    dotenv().ok();
    env_logger::init();

    log::info!("Starting TikTok to Telegram Bot (100% Pure Rust)...");

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
        if text.contains("tiktok.com") {
            if let Err(e) = handle_tiktok_link(bot, msg, text, downloader, semaphore).await {
                log::error!("Error processing TikTok request: {:?}", e);
            }
        } else if text.starts_with("/start") {
            let _ = bot
                .send_message(
                    msg.chat.id,
                    "👋 Send me any TikTok video link, and I will download and send it to you without watermarks!",
                )
                .await;
        }
    }
    Ok(())
}
    })
    .await;
}

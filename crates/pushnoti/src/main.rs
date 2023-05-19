use anyhow::{Context, Result};
use clap::Parser;
use fcm_push_listener::*;
use log::*;
use std::{fs, path::PathBuf};
use validator::Validate;

mod config;
mod opt;
mod utils;

use config::Config;
use opt::Opt;
use utils::find_and_connect;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let opt = Opt::parse();

    let conf_path = opt.config.context("Must supply --config file")?;
    let config: Config = toml::from_str(
        &fs::read_to_string(&conf_path)
            .with_context(|| format!("Failed to read {:?}", conf_path))?,
    )
    .with_context(|| format!("Failed to parse the {:?} config file", conf_path))?;

    config
        .validate()
        .with_context(|| format!("Failed to validate the {:?} config file", conf_path))?;

    let camera = find_and_connect(&config, &opt.camera).await?;

    // 696841269229 is the reo_iphone FCM Sender_ID
    // let registration = fcm_push_listener::register("696841269229").await?;
    // 743639030586 is the reo_fcm FCM Sender_ID
    // 263684512460 is my test Sender_ID
    // let registration = fcm_push_listener::register("743639030586").await?
    // I have confirmed that I can recieve test messages with this SenderID
    // into this program
    // let registration = fcm_push_listener::register("263684512460").await?;

    let sender_id = "743639030586"; // andriod

    // let sender_id = "696841269229"; // ios

    // let sender_id = "263684512460"; // test

    let token_path = PathBuf::from("./token.toml");
    let registration = if let Ok(Ok(registration)) =
        fs::read_to_string(&token_path).map(|v| toml::from_str::<Registration>(&v))
    {
        info!("Loaded token");
        registration
    } else {
        info!("Registering new token");
        let registration = fcm_push_listener::register(sender_id).await?;
        let new_token = toml::to_string(&registration)?;
        fs::write(token_path, new_token)?;
        registration
    };

    // Send registration.fcm_token to the server to allow it to send push messages to you.
    info!("registration.fcm_token: {}", registration.fcm_token);
    let uid = "6A5443E486511B0D828543445DC55A7D"; // MD5 Hash of "WHY_REOLINK"
    camera
        .send_pushinfo_android(&registration.fcm_token, uid)
        .await?;

    info!("Listening");
    let mut listener = FcmPushListener::create(
        registration,
        |message: FcmMessage| {
            info!("Message JSON: {}", message.payload_json);
            info!("Persistent ID: {:?}", message.persistent_id);
        },
        vec![],
    );
    listener.connect().await?;
    Ok(())
}

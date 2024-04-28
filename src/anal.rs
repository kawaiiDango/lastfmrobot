// from https://github.com/arsenron/amplitude

use std::{
    sync::{LazyLock, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use reqwest::StatusCode;
use serde::{Deserialize, Serialize};

use crate::{api_requester::CLIENT_NOCACHE, config};

#[derive(Serialize, Deserialize, Debug, Clone)]
struct UploadBody {
    pub api_key: String,
    pub events: Vec<Event>,
}

/// The main entity to send to the amplitude servers
///
/// [The official docs](https://developers.amplitude.com/docs/http-api-v2#schemaevent)
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
struct Event {
    event_type: Option<String>,
    user_id: Option<String>,
    platform: Option<String>,
    time: Option<u64>,
    language: Option<String>,
    ip: Option<String>,
}

const MAX_EVENTS_TO_TRIGGER_SEND: usize = 50; // todo increase later
const URL_BATCH: &str = "https://api2.amplitude.com/batch";
const DEFAULT_SERVER_ERROR: &str = r#"{"error": "Some kind of server error"}"#;
static EVENTS_BUFFER: LazyLock<Mutex<Vec<Event>>> = LazyLock::new(|| Mutex::new(Vec::new()));

pub async fn add_event(
    event_type: &str,
    user: Option<&teloxide::types::User>,
    bot_username: String,
) -> Result<(), reqwest_middleware::Error> {
    let user_id = user
        .cloned()
        .map(|x| x.id.0.to_string())
        .unwrap_or_default();
    let language_code = user.cloned().map(|x| x.language_code).unwrap_or_default();

    if let Ok(mut buffer) = EVENTS_BUFFER.lock() {
        buffer.push(Event {
            event_type: event_type.to_string().into(),
            user_id: user_id.into(),
            platform: bot_username.into(),
            time: Some(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64,
            ),
            language: language_code,
            ip: "$remote".to_string().into(),
        });
    }

    if EVENTS_BUFFER.lock().unwrap().len() > MAX_EVENTS_TO_TRIGGER_SEND {
        send().await?;

        if let Ok(mut buffer) = EVENTS_BUFFER.lock() {
            buffer.clear();
        }
    }
    Ok(())
}

/// Sends bunch of events to the amplitude servers
pub async fn send() -> Result<(), reqwest_middleware::Error> {
    let upload_body = UploadBody {
        api_key: config::AMPLITUDE_KEY.into(),
        events: EVENTS_BUFFER.lock().unwrap().clone(),
    };
    _send(&upload_body).await?;
    Ok(())
}

/// Sends an event to the amplitude servers
// pub async fn send_one(event: Event) -> Result<(), reqwest::Error> {
//     send(vec![event]).await
// }

async fn _send(upload_body: &UploadBody) -> Result<(), reqwest_middleware::Error> {
    let response = CLIENT_NOCACHE
        .post(URL_BATCH)
        .json(upload_body)
        .send()
        .await?;
    let status = response.status();
    let text = response.text().await.unwrap_or(DEFAULT_SERVER_ERROR.into());

    match status {
        StatusCode::OK => {}
        _ => {
            log::error!("{text}");
        }
    }

    Ok(())
}

use crate::handler;
use anyhow::{bail, Result};
use reqwest;
use serde::{Deserialize, Serialize};
use serde_json::{self, json};
use serde_with::{serde_as, Bytes};
#[cfg(any(target_os = "windows", target_os = "macos"))]
use std::fs;
use std::{fmt::Debug, future::Future};

#[cfg(any(target_os = "windows", target_os = "macos"))]
use anyhow::anyhow;
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EngineConfig {
    pub address: String,
}

macro_rules! cond_argument {
    ($data:expr, $key:literal, $option_value:ident) => {
        if let Some(value) = $option_value {
            $data
                .as_object_mut()
                .unwrap()
                .insert(String::from($key), json!(value));
        }
    };
}

#[derive(Clone, Debug)]
pub struct RawGameSenseClient {
    client: reqwest::Client,
    address: String,
}

pub trait EngineRequest {
    fn path() -> &'static str;
}

pub trait AsyncEngineRequest: Serialize + EngineRequest {}

pub trait Sendable {
    type ResultFuture<'a>: Future<Output = Result<String>> + 'a
    where
        Self: 'a;

    #[allow(clippy::needless_lifetimes)]
    fn send<'this>(&'this self, client: &'this RawGameSenseClient) -> Self::ResultFuture<'this>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoveGame<'b> {
    pub game: &'b str,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Heartbeat<'b> {
    pub game: &'b str,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoveEvent<'b> {
    pub game: &'b str,
    pub event: &'b str,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterGame<'b> {
    pub game: &'b str,
    #[serde(rename = "game_display_name", skip_serializing_if = "Option::is_none")]
    pub display_name: Option<&'b str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub developer: Option<&'b str>,
    #[serde(
        rename = "deinitialize_timer_length_ms",
        skip_serializing_if = "Option::is_none"
    )]
    pub timeout: Option<u32>,
}

pub trait GameEventData {}

impl<'a> GameEventData for FrameContainer<'a> {}

#[derive(Debug, Clone, Serialize, Default)]
pub struct FrameContainer<'a> {
    pub frame: ScreenFrameData<'a>,
}

impl GameEventData for () {}

#[serde_as]
#[derive(Debug, Clone, Serialize, Default)]
pub struct ScreenFrameData<'a> {
    #[serde_as(as = "Option<Bytes>")]
    #[serde(rename = "image-data-128x36")]
    pub image_128x36: Option<&'a [u8; 576]>,
    #[serde_as(as = "Option<Bytes>")]
    #[serde(rename = "image-data-128x40")]
    pub image_128x40: Option<&'a [u8; 640]>,
    #[serde_as(as = "Option<Bytes>")]
    #[serde(rename = "image-data-128x48")]
    pub image_128x48: Option<&'a [u8; 768]>,
    #[serde_as(as = "Option<Bytes>")]
    #[serde(rename = "image-data-128x52")]
    pub image_128x52: Option<&'a [u8; 852]>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GameEvent<'b, D: GameEventData> {
    pub game: &'b str,
    pub event: &'b str,
    pub data: D,
}

#[derive(Debug, Clone, Serialize)]
pub struct RegisterEvent<'b> {
    pub game: &'b str,
    pub event: &'b str,
    pub min_value: Option<u32>,
    pub max_value: Option<u32>,
    pub icon_id: Option<u32>,
    pub value_optional: Option<bool>,
}

impl<T: AsyncEngineRequest> Sendable for T {
    type ResultFuture<'a>
    where
        T: 'a,
    = impl Future<Output = Result<String>> + 'a;

    fn send<'this>(&'this self, client: &'this RawGameSenseClient) -> Self::ResultFuture<'this> {
        async move {
            let value = serde_json::to_value(self)?;
            client.send_data(Self::path(), &value).await
        }
    }
}

macro_rules! engine_request {
    ($target:ty, $lt:lifetime,$name:literal) => {
        impl<$lt> EngineRequest for $target {
            fn path() -> &'static str {
                $name
            }
        }
        impl<$lt> AsyncEngineRequest for $target {}
    };
    ($target:ty, $name:literal) => {
        impl EngineRequest for $target {
            fn path() -> &'static str {
                $name
            }
        }
        impl AsyncEngineRequest for $target {}
    };
}

impl<'a, T: GameEventData + Serialize> EngineRequest for GameEvent<'a, T> {
    fn path() -> &'static str {
        "game_event"
    }
}

impl<'b, T: GameEventData + Serialize> AsyncEngineRequest for GameEvent<'b, T> {}

engine_request!(RemoveGame<'b>,'b, "remove_game");
engine_request!(RemoveEvent<'b>,'b, "remove_game_event");
engine_request!(RegisterGame<'b>,'b, "game_metadata");
engine_request!(RegisterEvent<'b>,'b, "register_game_event");
engine_request!(Heartbeat<'b>,'b, "game_heartbeat");

impl RawGameSenseClient {
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    pub fn new() -> Result<RawGameSenseClient> {
        Ok(RawGameSenseClient {
            client: reqwest::Client::new(),
            address: "127.0.0.1:5000".to_owned(),
        })
    }

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    pub fn new() -> Result<RawGameSenseClient> {
        #[cfg(target_os = "macos")]
        let path = "/Library/Application Support/SteelSeries Engine 3/coreProps.json";

        #[cfg(target_os = "windows")]
        let path =
            std::env::var("PROGRAMDATA")? + "/SteelSeries/SteelSeries Engine 3/coreProps.json";

        let config = fs::read_to_string(path)?;
        let config = serde_json::from_str::<EngineConfig>(&config)?;

        Ok(RawGameSenseClient {
            client: reqwest::Client::new(),
            address: config.address,
        })
    }

    pub async fn send_data(&self, endpoint: &str, data: &serde_json::Value) -> Result<String> {
        let data = self
            .client
            .post(format!("http://{}/{}", self.address, endpoint))
            .json(data)
            .send()
            .await?
            .text()
            .await?;

        if data == "Page not found" {
            bail!("Endpoint not found");
        }

        let data: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&data)?;

        let (key, value) = data.iter().next().unwrap();
        let value = value.as_str().unwrap_or(&value.to_string()).to_owned();

        match key.as_str() {
            "error" => bail!(value),
            _ => Ok(value),
        }
    }

    pub async fn game_event(
        &self,
        game: &str,
        event: &str,
        value: isize,
        frame: Option<serde_json::Value>,
    ) -> Result<String> {
        let mut data = json!({
            "game": game,
            "event": event,
            "data": {
                "value": value
            }
        });

        cond_argument!(data.get_mut("data").unwrap(), "frame", frame);

        self.send_data("game_event", &data).await
    }

    pub async fn heartbeat(&self, game: &str) -> Result<String> {
        let data = json!({ "game": game });

        self.send_data("game_heartbeat", &data).await
    }

    pub async fn register_game(
        &self,
        game: &str,
        game_display_name: Option<&str>,
        developer: Option<&str>,
        deinitialize_timer_length_ms: Option<u32>,
    ) -> Result<String> {
        let data = RegisterGame {
            game,
            display_name: game_display_name,
            developer,
            timeout: deinitialize_timer_length_ms,
        };

        data.send(self).await
    }

    pub async fn remove_game(&self, game: &str) -> Result<String> {
        let data = RemoveGame { game };

        data.send(self).await
    }

    pub async fn bind_event<T: Serialize + handler::Handler>(
        &self,
        game: &str,
        event: &str,
        min_value: Option<isize>,
        max_value: Option<isize>,
        icon_id: Option<u8>,
        value_optional: Option<bool>,
        handlers: Vec<T>,
    ) -> Result<String> {
        let mut data = json!({
            "game": game,
            "event": event,
            "handlers": handlers
        });

        cond_argument!(data, "min_value", min_value);
        cond_argument!(data, "max_value", max_value);
        cond_argument!(data, "icon_id", icon_id);
        cond_argument!(data, "value_optional", value_optional);

        self.send_data("bind_game_event", &data).await
    }

    pub async fn register_event(
        &self,
        game: &str,
        event: &str,
        min_value: Option<isize>,
        max_value: Option<isize>,
        icon_id: Option<u8>,
        value_optional: Option<bool>,
    ) -> Result<String> {
        let mut data = json!({
            "game": game,
            "event": event
        });

        cond_argument!(data, "min_value", min_value);
        cond_argument!(data, "max_value", max_value);
        cond_argument!(data, "icon_id", icon_id);
        cond_argument!(data, "value_optional", value_optional);

        self.send_data("register_game_event", &data).await
    }

    pub async fn remove_event(&self, game: &str, event: &str) -> Result<String> {
        let data = RemoveEvent { game, event };

        data.send(self).await
    }
}

use crate::{handler, raw_client::RawGameSenseClient};
use anyhow::{Context, Result};
use serde::Serialize;
use serde_json;
use std::{sync::Arc, time::Duration};
use tokio::{task::JoinHandle, time::MissedTickBehavior};

#[derive(Debug)]
pub struct GameSenseClient {
    raw_client: Arc<RawGameSenseClient>,
    game: String,
    heartbeat: Option<JoinHandle<()>>,
}

impl GameSenseClient {
    pub async fn new(
        game: &str,
        game_display_name: &str,
        developer: &str,
        deinitialize_timer_length_ms: Option<u32>,
    ) -> Result<GameSenseClient> {
        let client = Self::from_game_name(game)?;

        client.raw_client.remove_game(&client.game).await.ok();
        client
            .raw_client
            .register_game(
                &client.game,
                Some(game_display_name),
                Some(developer),
                deinitialize_timer_length_ms,
            )
            .await?;

        Ok(client)
    }

    pub fn from_game_name(game: &str) -> Result<GameSenseClient> {
        Ok(GameSenseClient {
            raw_client: Arc::new(RawGameSenseClient::new()?),
            game: game.to_owned(),
            heartbeat: None,
        })
    }

    pub fn start_heartbeat(&mut self) {
        let mut interval = tokio::time::interval(Duration::from_secs(10));
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

        let client = self.raw_client.clone();
        let game = self.game.clone();

        self.heartbeat = Some(tokio::spawn(async move {
            loop {
                interval.tick().await;
                client.heartbeat(&game).await.ok();
            }
        }));
    }

    pub fn stop_heartbeat(&mut self) -> Result<()> {
        Ok(self
            .heartbeat
            .as_mut()
            .context("Trying to stop uninitialized heartbeat thread")?
            .abort())
    }

    pub async fn bind_event<T: Serialize + handler::Handler>(
        &self,
        event: &str,
        min_value: Option<isize>,
        max_value: Option<isize>,
        icon_id: Option<u8>,
        value_optional: Option<bool>,
        handlers: Vec<T>,
    ) -> Result<String> {
        self.raw_client
            .bind_event(
                &self.game,
                event,
                min_value,
                max_value,
                icon_id,
                value_optional,
                handlers,
            )
            .await
    }

    pub async fn register_event(&self, event: &str) -> Result<String> {
        self.register_event_full(event, None, None, None, None)
            .await
    }

    pub async fn register_event_full(
        &self,
        event: &str,
        min_value: Option<isize>,
        max_value: Option<isize>,
        icon_id: Option<u8>,
        value_optional: Option<bool>,
    ) -> Result<String> {
        // self.remove_event(event).ok();
        self.raw_client
            .register_event(
                &self.game,
                event,
                min_value,
                max_value,
                icon_id,
                value_optional,
            )
            .await
    }

    pub async fn remove_event(&self, event: &str) -> Result<String> {
        self.raw_client.remove_event(&self.game, event).await
    }

    pub async fn trigger_event(&self, event: &str, value: isize) -> Result<String> {
        self.raw_client
            .game_event(&self.game, event, value, None)
            .await
    }

    pub async fn trigger_event_frame(
        &self,
        event: &str,
        value: isize,
        frame: serde_json::Value,
    ) -> Result<String> {
        self.raw_client
            .game_event(&self.game, event, value, Some(frame))
            .await
    }
}

impl Drop for GameSenseClient {
    fn drop(&mut self) {
        self.stop_heartbeat().ok();
    }
}

use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::Serialize;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use crate::browser::GameBrowser;
use crate::config::Config;

#[derive(Debug, Clone, Serialize)]
pub struct MercExchange {
    pub kingdom: u32,
    pub x: u32,
    pub y: u32,
    pub found_at: DateTime<Utc>,
}

pub struct AppStateInner {
    pub running: bool,
    pub current_kingdom: Option<u32>,
    pub exchanges: Vec<MercExchange>,
    pub scanner_handle: Option<JoinHandle<()>>,
    pub config: Config,
    pub browser: Option<Arc<GameBrowser>>,
}

pub type AppState = Arc<Mutex<AppStateInner>>;

impl AppStateInner {
    pub fn new(config: Config) -> Self {
        Self {
            running: false,
            current_kingdom: None,
            exchanges: Vec::new(),
            scanner_handle: None,
            config,
            browser: None,
        }
    }

    /// Add exchange with deduplication: skip if same K/X/Y was found within last 5 minutes.
    /// Caps total exchanges at the number of configured kingdoms.
    pub fn add_exchange(&mut self, exchange: MercExchange) -> bool {
        let now = Utc::now();
        let five_min = chrono::Duration::minutes(5);

        let is_duplicate = self.exchanges.iter().any(|e| {
            e.kingdom == exchange.kingdom
                && e.x == exchange.x
                && e.y == exchange.y
                && (now - e.found_at) < five_min
        });

        if is_duplicate {
            return false;
        }

        if self.exchanges.len() >= self.config.kingdoms.len() {
            return false;
        }

        self.exchanges.push(exchange);
        true
    }

    pub fn is_full(&self) -> bool {
        self.exchanges.len() >= self.config.kingdoms.len()
    }
}

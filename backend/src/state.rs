use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::Serialize;
use tokio::sync::{Mutex, Notify};
use tokio::task::JoinHandle;

use crate::browser::GameBrowser;
use crate::config::Config;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScannerPhase {
    Idle,
    Preparing,
    Ready,
    Scanning,
    Paused,
}

#[derive(Debug, Clone, Serialize)]
pub struct MercExchange {
    pub kingdom: u32,
    pub x: u32,
    pub y: u32,
    pub found_at: DateTime<Utc>,
    /// How long the scan took to find this exchange (seconds).
    pub scan_duration_secs: Option<f64>,
    /// true = coordinates parsed from popup, false = calibration estimate.
    pub confirmed: bool,
    /// Screenshot taken after clicking the match (PNG bytes).
    #[serde(skip)]
    pub screenshot_png: Option<Vec<u8>>,
}

pub struct AppStateInner {
    pub phase: ScannerPhase,
    pub current_kingdom: Option<u32>,
    pub exchanges: Vec<MercExchange>,
    pub scanner_handle: Option<JoinHandle<()>>,
    pub config: Config,
    pub browser: Option<Arc<GameBrowser>>,
    pub pause_notify: Arc<Notify>,
    pub last_kingdom_scan: HashMap<u32, DateTime<Utc>>,
    /// Last screenshot taken (by goto or refresh), reused by detect.
    pub last_screenshot: Option<Vec<u8>>,
}

pub type AppState = Arc<Mutex<AppStateInner>>;

impl AppStateInner {
    pub fn new(config: Config) -> Self {
        Self {
            phase: ScannerPhase::Idle,
            current_kingdom: None,
            exchanges: Vec::new(),
            scanner_handle: None,
            config,
            browser: None,
            pause_notify: Arc::new(Notify::new()),
            last_kingdom_scan: HashMap::new(),
            last_screenshot: None,
        }
    }

    /// Add exchange with deduplication: skip if same K/X/Y was found within last 5 minutes.
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

        self.exchanges.push(exchange);
        true
    }

    pub fn last_scan_time(&self, kingdom: u32) -> Option<DateTime<Utc>> {
        self.last_kingdom_scan.get(&kingdom).copied()
    }

    pub fn set_last_scan_time(&mut self, kingdom: u32) {
        self.last_kingdom_scan.insert(kingdom, Utc::now());
    }

    /// Return (x, y) of the most recent exchange found for a given kingdom.
    pub fn exchange_for_kingdom(&self, kingdom: u32) -> Option<(u32, u32)> {
        self.exchanges
            .iter()
            .filter(|e| e.kingdom == kingdom)
            .max_by_key(|e| e.found_at)
            .map(|e| (e.x, e.y))
    }

    /// Update `found_at` to now for the matching exchange.
    pub fn refresh_exchange(&mut self, kingdom: u32, x: u32, y: u32) {
        if let Some(e) = self
            .exchanges
            .iter_mut()
            .find(|e| e.kingdom == kingdom && e.x == x && e.y == y)
        {
            e.found_at = Utc::now();
        }
    }

    /// Remove all exchanges for a given kingdom.
    pub fn remove_exchange(&mut self, kingdom: u32) {
        self.exchanges.retain(|e| e.kingdom != kingdom);
    }
}

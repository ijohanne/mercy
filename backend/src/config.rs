use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("missing environment variable: {0}")]
    MissingEnv(String),

    #[error("invalid kingdoms list: {0}")]
    InvalidKingdoms(String),
}

#[derive(Debug, Clone)]
pub struct Config {
    pub kingdoms: Vec<u32>,
    pub auth_token: String,
    pub tb_email: String,
    pub tb_password: String,
    pub listen_addr: String,
    pub chromium_path: Option<String>,
    /// Run browser in headless mode (default false; use xvfb-run on servers)
    pub headless: bool,
    /// Name of the tile to search for in popup confirmation (e.g. "Taotie", "Mercenary Exchange")
    pub search_target: String,
    /// Write debug screenshots to disk every scan step (default false)
    pub debug_screenshots: bool,
    /// Fly-animation wait after navigate_to_coords, in milliseconds (default 2000)
    pub navigate_delay_ms: u64,
    /// Scan pattern: "single", "multi", "wide", "grid" (default "grid")
    pub scan_pattern: String,
    /// Override ring count per pattern (None = use pattern default)
    pub scan_rings: Option<u32>,
    /// Path to exchange JSONL log file (default "exchanges.jsonl")
    pub exchange_log: String,
    /// Coverage percentage for "known" scan pattern (1-100, default 80).
    /// Lower values scan fewer positions (faster) but may miss exchanges
    /// in historically rare spawn locations.
    pub known_coverage: u32,
    /// Max concurrent detection tasks (default 4)
    pub max_detect_tasks: usize,
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let kingdoms_str = required_env("MERCY_KINGDOMS")?;
        let kingdoms: Vec<u32> = kingdoms_str
            .split(',')
            .map(|s| {
                s.trim()
                    .parse::<u32>()
                    .map_err(|e| ConfigError::InvalidKingdoms(format!("{s}: {e}")))
            })
            .collect::<Result<Vec<_>, _>>()?;

        if kingdoms.is_empty() {
            return Err(ConfigError::InvalidKingdoms(
                "at least one kingdom required".into(),
            ));
        }

        let auth_token = required_env("MERCY_AUTH_TOKEN")?;
        let tb_email = required_env("MERCY_TB_EMAIL")?;
        let tb_password = required_env("MERCY_TB_PASSWORD")?;

        let listen_addr =
            std::env::var("MERCY_LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0:8090".into());

        let chromium_path = std::env::var("MERCY_CHROMIUM_PATH").ok();

        let headless = std::env::var("MERCY_HEADLESS")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);

        let search_target = std::env::var("MERCY_SEARCH_TARGET")
            .unwrap_or_else(|_| "Mercenary Exchange Core".into());

        let debug_screenshots = std::env::var("MERCY_DEBUG_SCREENSHOTS")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);

        let navigate_delay_ms = std::env::var("MERCY_NAVIGATE_DELAY_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(750);

        let scan_pattern = std::env::var("MERCY_SCAN_PATTERN").unwrap_or_else(|_| "grid".into());

        let scan_rings = std::env::var("MERCY_SCAN_RINGS")
            .ok()
            .and_then(|v| v.parse().ok());

        let exchange_log =
            std::env::var("MERCY_EXCHANGE_LOG").unwrap_or_else(|_| "exchanges.jsonl".into());

        let known_coverage = std::env::var("MERCY_KNOWN_COVERAGE")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(80u32)
            .clamp(1, 100);

        let max_detect_tasks = std::env::var("MERCY_MAX_DETECT_TASKS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(4);

        Ok(Config {
            kingdoms,
            auth_token,
            tb_email,
            tb_password,
            listen_addr,
            chromium_path,
            headless,
            search_target,
            debug_screenshots,
            navigate_delay_ms,
            scan_pattern,
            scan_rings,
            exchange_log,
            known_coverage,
            max_detect_tasks,
        })
    }
}

fn required_env(name: &str) -> Result<String, ConfigError> {
    std::env::var(name).map_err(|_| ConfigError::MissingEnv(name.into()))
}

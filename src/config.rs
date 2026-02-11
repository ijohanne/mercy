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

        let listen_addr = std::env::var("MERCY_LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0:8090".into());

        let chromium_path = std::env::var("MERCY_CHROMIUM_PATH").ok();

        let headless = std::env::var("MERCY_HEADLESS")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);

        let search_target =
            std::env::var("MERCY_SEARCH_TARGET").unwrap_or_else(|_| "Mercenary Exchange".into());

        Ok(Config {
            kingdoms,
            auth_token,
            tb_email,
            tb_password,
            listen_addr,
            chromium_path,
            headless,
            search_target,
        })
    }
}

fn required_env(name: &str) -> Result<String, ConfigError> {
    std::env::var(name).map_err(|_| ConfigError::MissingEnv(name.into()))
}

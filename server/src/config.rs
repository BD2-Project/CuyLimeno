use std::{env, net::SocketAddr, time::Duration};

#[derive(Clone)]
pub struct Config {
    pub listen: SocketAddr,
    pub database_host: String,
    pub database_port: u16,
    pub database_timeout: Duration,
    pub groq_configured: bool,
    pub groq_model: Option<String>,
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        let listen = value("CUYLIMENO_SERVER_ADDR", "127.0.0.1:3001")
            .parse::<SocketAddr>()
            .map_err(|_| "CUYLIMENO_SERVER_ADDR debe ser una dirección IP:puerto".to_owned())?;
        let database_host = value("SOUPDB_HOST", "127.0.0.1");
        if database_host.trim().is_empty() {
            return Err("SOUPDB_HOST no puede estar vacío".to_owned());
        }
        let database_port = value("SOUPDB_PORT", "55432")
            .parse::<u16>()
            .ok()
            .filter(|port| *port > 0)
            .ok_or("SOUPDB_PORT debe ser un puerto entre 1 y 65535")?;
        let timeout_ms = value("SOUPDB_TIMEOUT_MS", "3000")
            .parse::<u64>()
            .ok()
            .filter(|timeout| (1..=60_000).contains(timeout))
            .ok_or("SOUPDB_TIMEOUT_MS debe estar entre 1 y 60000")?;
        Ok(Self {
            listen,
            database_host,
            database_port,
            database_timeout: Duration::from_millis(timeout_ms),
            groq_configured: env::var("GROQ_API_KEY").is_ok_and(|key| !key.trim().is_empty())
                && env::var("GROQ_MODEL").is_ok_and(|model| !model.trim().is_empty()),
            groq_model: env::var("GROQ_MODEL")
                .ok()
                .filter(|model| !model.trim().is_empty()),
        })
    }
}

fn value(name: &str, default: &str) -> String {
    env::var(name).unwrap_or_else(|_| default.to_owned())
}


use std::{env, net::SocketAddr, path::PathBuf, time::Duration};

#[derive(Clone)]
pub struct Config {
    pub listen: SocketAddr,
    pub database_host: String,
    pub database_port: u16,
    pub database_timeout: Duration,
    pub pool_size: usize,
    pub data_dir: PathBuf,
    pub inverted_storage_root: String,
    pub groq_api_key: Option<String>,
    pub groq_model: Option<String>,
    pub groq_base_url: String,
    pub groq_timeout: Duration,
    pub max_upload_bytes: u64,
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        load_dotenv();
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
        let timeout_ms = parse_ranged("SOUPDB_TIMEOUT_MS", "10000", 1, 120_000)?;
        let pool_size = parse_ranged("SOUPDB_POOL_SIZE", "4", 1, 32)? as usize;
        let groq_timeout_ms = parse_ranged("GROQ_TIMEOUT_MS", "120000", 1_000, 600_000)?;
        let max_upload_mb = parse_ranged("CUYLIMENO_MAX_UPLOAD_MB", "50", 1, 500)?;
        let data_dir = PathBuf::from(value("CUYLIMENO_DATA_DIR", "./data/cuylimeno"));
        let inverted_storage_root = value(
            "SOUPDB_INVERTED_STORAGE_ROOT",
            "/app/data/plugins/inverted",
        );
        let groq_api_key = non_empty("GROQ_API_KEY");
        let groq_model = non_empty("GROQ_MODEL");
        let groq_base_url = value("GROQ_API_BASE_URL", "https://api.groq.com/openai/v1")
            .trim_end_matches('/')
            .to_owned();
        Ok(Self {
            listen,
            database_host,
            database_port,
            database_timeout: Duration::from_millis(timeout_ms),
            pool_size,
            data_dir,
            inverted_storage_root,
            groq_api_key,
            groq_model,
            groq_base_url,
            groq_timeout: Duration::from_millis(groq_timeout_ms),
            max_upload_bytes: max_upload_mb * 1024 * 1024,
        })
    }
}

fn value(name: &str, default: &str) -> String {
    env::var(name).unwrap_or_else(|_| default.to_owned())
}

/// Carga `.env` para desarrollo sin sobreescribir variables ya exportadas al
/// proceso. El archivo está en `.gitignore`; nunca se versiona la API key.
fn load_dotenv() {
    for candidate in [".env", "../.env", "server/.env"] {
        let path = std::path::Path::new(candidate);
        if path.is_file() {
            let _ = dotenvy::from_path(path);
        }
    }
}

fn non_empty(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn parse_ranged(name: &str, default: &str, min: u64, max: u64) -> Result<u64, String> {
    value(name, default)
        .parse::<u64>()
        .ok()
        .filter(|value| (min..=max).contains(value))
        .ok_or_else(|| format!("{name} debe ser un número entre {min} y {max}"))
}

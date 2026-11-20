pub mod chat;
pub mod collections;
pub mod conversations;
pub mod documents;
pub mod settings;

use axum::{extract::State, http::StatusCode, Json};
use serde::Serialize;

use crate::{db::DbError, error::ApiError, state::SharedState};

#[derive(Serialize)]
pub struct Health {
    status: &'static str,
    database: &'static str,
    driver: &'static str,
}

#[derive(Serialize)]
pub struct Capabilities {
    driver: &'static str,
    health_check: &'static str,
    schema_configured: bool,
    ingestion_available: bool,
    retrieval_available: bool,
    generation_available: bool,
    authentication_available: bool,
    groq_configured: bool,
    groq_model: Option<String>,
}

pub async fn health(State(state): State<SharedState>) -> Result<Json<Health>, ApiError> {
    match state.db.ping().await {
        Ok(()) => Ok(Json(Health {
            status: "ok",
            database: "connected",
            driver: "rsoup",
        })),
        Err(DbError::Timeout) => Err(ApiError::service_unavailable(
            "DATABASE_TIMEOUT",
            "SoupDB no respondió dentro del tiempo configurado.",
        )),
        Err(error) => {
            eprintln!("SoupDB health check failed: {error}");
            Err(ApiError::service_unavailable(
                "DATABASE_UNAVAILABLE",
                "No se pudo conectar con SoupDB mediante rsoup.",
            ))
        }
    }
}

pub async fn capabilities(State(state): State<SharedState>) -> Json<Capabilities> {
    let schema_ready = state.schema_ready();
    let model = state.active_model();
    let key_configured = state.config.groq_api_key.is_some();
    Json(Capabilities {
        driver: "rsoup",
        health_check: "rsoup_ping",
        schema_configured: schema_ready,
        ingestion_available: schema_ready,
        retrieval_available: state.index_ready(),
        generation_available: key_configured && model.is_some() && schema_ready,
        authentication_available: false,
        groq_configured: key_configured && model.is_some(),
        groq_model: model,
    })
}

pub async fn not_found() -> ApiError {
    ApiError::not_found("NOT_FOUND", "El recurso solicitado no existe.")
}

pub async fn method_not_allowed() -> ApiError {
    ApiError::new(
        StatusCode::METHOD_NOT_ALLOWED,
        "METHOD_NOT_ALLOWED",
        "El método solicitado no está permitido.",
    )
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{atomic::AtomicBool, Arc, RwLock},
        time::Duration,
    };

    use axum::body::to_bytes;
    use axum::response::IntoResponse;
    use rsoup::protocol::{Frame, HEADER_SIZE, OP_PING, OP_PONG};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    use super::*;
    use crate::{config::Config, db::Db, state::AppState};

    fn config(port: u16) -> Config {
        Config {
            listen: "127.0.0.1:3001".parse().unwrap(),
            database_host: "127.0.0.1".to_owned(),
            database_port: port,
            database_timeout: Duration::from_millis(100),
            pool_size: 1,
            data_dir: std::env::temp_dir(),
            inverted_storage_root: "/tmp/inverted".to_owned(),
            groq_api_key: None,
            groq_model: None,
            groq_base_url: "https://api.groq.com/openai/v1".to_owned(),
            groq_timeout: Duration::from_secs(5),
            max_upload_bytes: 1024,
        }
    }

    fn state(port: u16) -> SharedState {
        let config = config(port);
        Arc::new(AppState {
            db: Db::new(&config),
            config,
            schema_ready: AtomicBool::new(false),
            index_ready: AtomicBool::new(false),
            active_model: RwLock::new(None),
        })
    }

    #[tokio::test]
    async fn health_requires_real_driver_ping_pong() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let peer = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0u8; HEADER_SIZE];
            socket.read_exact(&mut request).await.unwrap();
            assert_eq!(&request[..2], b"SP");
            assert_eq!(request[3], OP_PING);
            socket
                .write_all(
                    &Frame {
                        opcode: OP_PONG,
                        payload: vec![],
                    }
                    .encode(),
                )
                .await
                .unwrap();
        });
        let response = health(State(state(port))).await;
        assert!(matches!(
            response,
            Ok(Json(Health {
                database: "connected",
                ..
            }))
        ));
        peer.await.unwrap();
    }

    #[tokio::test]
    async fn health_times_out_when_database_does_not_answer() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let result = health(State(state(listener.local_addr().unwrap().port()))).await;
        match result {
            Err(error) => {
                assert_eq!(error.status, StatusCode::SERVICE_UNAVAILABLE);
                assert_eq!(error.code, "DATABASE_TIMEOUT");
            }
            Ok(_) => panic!("An unanswered ping must not report readiness"),
        }
    }

    #[tokio::test]
    async fn missing_schema_returns_explicit_error_instead_of_empty_records() {
        let error = state(55432)
            .require_schema()
            .expect_err("el esquema no está configurado");
        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
        let body = to_bytes(response.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"]["code"], "DOCUMENT_SCHEMA_NOT_CONFIGURED");
    }

    #[tokio::test]
    async fn groq_configuration_does_not_imply_available_generation() {
        let mut settings = config(55432);
        settings.groq_api_key = Some("test-key".to_owned());
        settings.groq_model = Some("configured-model".to_owned());
        let shared = Arc::new(AppState {
            db: Db::new(&settings),
            active_model: RwLock::new(settings.groq_model.clone()),
            config: settings,
            schema_ready: AtomicBool::new(false),
            index_ready: AtomicBool::new(false),
        });
        let Json(capabilities) = capabilities(State(shared)).await;
        assert!(capabilities.groq_configured);
        assert!(!capabilities.schema_configured);
        assert!(!capabilities.generation_available);
    }

    #[tokio::test]
    async fn schema_ready_implies_ingestion_and_retrieval_but_needs_groq_for_generation() {
        let settings = config(55432);
        let shared = Arc::new(AppState {
            db: Db::new(&settings),
            config: settings,
            schema_ready: AtomicBool::new(true),
            index_ready: AtomicBool::new(true),
            active_model: RwLock::new(None),
        });
        let Json(capabilities) = capabilities(State(shared)).await;
        assert!(capabilities.schema_configured);
        assert!(capabilities.ingestion_available);
        assert!(capabilities.retrieval_available);
        assert!(!capabilities.generation_available);
    }

    #[tokio::test]
    async fn esquema_sin_indice_no_reporta_retrieval() {
        let settings = config(55432);
        let shared = Arc::new(AppState {
            db: Db::new(&settings),
            config: settings,
            schema_ready: AtomicBool::new(true),
            index_ready: AtomicBool::new(false),
            active_model: RwLock::new(None),
        });
        let Json(capabilities) = capabilities(State(shared)).await;
        assert!(capabilities.schema_configured);
        assert!(!capabilities.retrieval_available);
    }
}

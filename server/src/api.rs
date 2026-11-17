use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use rsoup::Connection;
use serde::Serialize;
use tokio::time::timeout;

use crate::config::Config;

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

#[derive(Serialize)]
struct ErrorEnvelope {
    error: ErrorDetails,
}

#[derive(Serialize)]
struct ErrorDetails {
    code: &'static str,
    message: &'static str,
}

pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: &'static str,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        (
            self.status,
            Json(ErrorEnvelope {
                error: ErrorDetails {
                    code: self.code,
                    message: self.message,
                },
            }),
        )
            .into_response()
    }
}

pub async fn health(State(config): State<Config>) -> Result<Json<Health>, ApiError> {
    let result = timeout(config.database_timeout, async {
        let mut connection =
            Connection::connect(&config.database_host, config.database_port).await?;
        connection.ping().await
    })
    .await;

    match result {
        Ok(Ok(())) => Ok(Json(Health {
            status: "ok",
            database: "connected",
            driver: "rsoup",
        })),
        Ok(Err(error)) => {
            eprintln!("SoupDB health check failed: {error}");
            Err(ApiError {
                status: StatusCode::SERVICE_UNAVAILABLE,
                code: "DATABASE_UNAVAILABLE",
                message: "No se pudo conectar con SoupDB mediante rsoup.",
            })
        }
        Err(_) => Err(ApiError {
            status: StatusCode::SERVICE_UNAVAILABLE,
            code: "DATABASE_TIMEOUT",
            message: "SoupDB no respondió dentro del tiempo configurado.",
        }),
    }
}

pub async fn capabilities(State(config): State<Config>) -> Json<Capabilities> {
    Json(Capabilities {
        driver: "rsoup",
        health_check: "rsoup_ping",
        schema_configured: false,
        ingestion_available: false,
        retrieval_available: false,
        generation_available: false,
        authentication_available: false,
        groq_configured: config.groq_configured,
        groq_model: config.groq_model,
    })
}

pub async fn schema_not_configured() -> ApiError {
    ApiError {
        status: StatusCode::NOT_IMPLEMENTED,
        code: "DOCUMENT_SCHEMA_NOT_CONFIGURED",
        message: "El esquema documental y la ingestión todavía no están configurados. No hay fuentes disponibles para generar respuestas.",
    }
}

pub async fn not_found() -> ApiError {
    ApiError {
        status: StatusCode::NOT_FOUND,
        code: "NOT_FOUND",
        message: "El recurso solicitado no existe.",
    }
}

pub async fn method_not_allowed() -> ApiError {
    ApiError {
        status: StatusCode::METHOD_NOT_ALLOWED,
        code: "METHOD_NOT_ALLOWED",
        message: "El método solicitado no está permitido.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use rsoup::protocol::{Frame, HEADER_SIZE, OP_PING, OP_PONG};
    use std::time::Duration;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    fn config(port: u16) -> Config {
        Config {
            listen: "127.0.0.1:3001".parse().unwrap(),
            database_host: "127.0.0.1".to_owned(),
            database_port: port,
            database_timeout: Duration::from_millis(100),
            groq_configured: false,
            groq_model: None,
        }
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
        let response = health(State(config(port))).await;
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
        let result = health(State(config(listener.local_addr().unwrap().port()))).await;
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
        let response = schema_not_configured().await.into_response();
        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
        let body = to_bytes(response.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"]["code"], "DOCUMENT_SCHEMA_NOT_CONFIGURED");
    }

    #[tokio::test]
    async fn groq_configuration_does_not_imply_available_generation() {
        let mut settings = config(55432);
        settings.groq_configured = true;
        settings.groq_model = Some("configured-model".to_owned());
        let Json(capabilities) = capabilities(State(settings)).await;
        assert!(capabilities.groq_configured);
        assert!(!capabilities.schema_configured);
        assert!(!capabilities.generation_available);
    }
}


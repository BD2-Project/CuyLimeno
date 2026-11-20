mod api;
mod config;
mod contracts;
mod db;
mod error;
mod ingest;
mod llm;
mod retrieval;
mod state;

use std::{
    sync::{atomic::AtomicBool, Arc, RwLock},
    time::Duration,
};

use axum::{
    extract::DefaultBodyLimit,
    routing::{get, put},
    Router,
};
use config::Config;
use db::Db;
use state::AppState;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_env().map_err(std::io::Error::other)?;
    tokio::fs::create_dir_all(&config.data_dir).await?;
    let listen = config.listen;
    let body_limit = config.max_upload_bytes as usize + 8 * 1024 * 1024;

    let initial_model = config.groq_model.clone();
    let state = Arc::new(AppState {
        db: Db::new(&config),
        config,
        schema_ready: AtomicBool::new(false),
        index_ready: AtomicBool::new(false),
        active_model: RwLock::new(initial_model),
    });
    spawn_bootstrap(state.clone());

    let app = Router::new()
        .route("/api/health", get(api::health))
        .route("/api/capabilities", get(api::capabilities))
        .route(
            "/api/documents",
            get(api::documents::list).post(api::documents::create),
        )
        .route(
            "/api/documents/{id}",
            get(api::documents::get).delete(api::documents::delete),
        )
        .route("/api/documents/{id}/content", get(api::documents::content))
        .route("/api/documents/{id}/file", get(api::documents::file))
        .route(
            "/api/collections",
            get(api::collections::list).post(api::collections::create),
        )
        .route(
            "/api/collections/{id}",
            get(api::collections::get)
                .patch(api::collections::update)
                .delete(api::collections::delete),
        )
        .route(
            "/api/collections/{id}/documents",
            axum::routing::post(api::collections::add_documents),
        )
        .route(
            "/api/collections/{id}/documents/{document_id}",
            axum::routing::delete(api::collections::remove_document),
        )
        .route(
            "/api/conversations",
            get(api::conversations::list).post(api::conversations::create),
        )
        .route(
            "/api/conversations/{id}",
            get(api::conversations::get)
                .patch(api::conversations::rename)
                .delete(api::conversations::delete),
        )
        .route("/api/chat", axum::routing::post(api::chat::chat))
        .route("/api/models", get(api::settings::models))
        .route("/api/settings/model", put(api::settings::set_model))
        .fallback(api::not_found)
        .method_not_allowed_fallback(api::method_not_allowed)
        .layer(DefaultBodyLimit::max(body_limit))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(listen).await?;
    println!("CuyLimeno API: http://{}", listener.local_addr()?);
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            if let Err(error) = tokio::signal::ctrl_c().await {
                eprintln!("No se pudo registrar la señal de cierre: {error}");
            }
        })
        .await?;
    Ok(())
}

fn spawn_bootstrap(state: state::SharedState) {
    tokio::spawn(async move {
        for attempt in 1..=10 {
            match state.db.bootstrap(&state.config).await {
                Ok(index_ready) => {
                    state.mark_schema_ready();
                    if index_ready {
                        state.mark_index_ready();
                    }
                    if let Ok(Some(model)) = state.db.meta_value("active_model").await {
                        state.set_active_model(model);
                    }
                    println!(
                        "CuyLimeno: esquema documental listo (intento {attempt}, \
                         índice INVERTED {})",
                        if index_ready { "operativo" } else { "PENDIENTE" }
                    );
                    return;
                }
                Err(error) => {
                    eprintln!("CuyLimeno: bootstrap {attempt}/10 falló: {error}");
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        }
        eprintln!(
            "CuyLimeno: no se pudo preparar el esquema documental; \
             los endpoints documentales responderán 501."
        );
    });
}

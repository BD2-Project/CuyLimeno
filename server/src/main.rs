mod api;
mod config;
pub mod contracts;

use axum::{routing::get, Router};
use config::Config;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_env().map_err(std::io::Error::other)?;
    let listener = tokio::net::TcpListener::bind(config.listen).await?;
    println!("CuyLimeno API: http://{}", listener.local_addr()?);
    let app = Router::new()
        .route("/api/health", get(api::health))
        .route("/api/capabilities", get(api::capabilities))
        .route(
            "/api/documents",
            get(api::schema_not_configured).post(api::schema_not_configured),
        )
        .route(
            "/api/documents/{id}",
            get(api::schema_not_configured).delete(api::schema_not_configured),
        )
        .route(
            "/api/documents/{id}/content",
            get(api::schema_not_configured),
        )
        .route(
            "/api/collections",
            get(api::schema_not_configured).post(api::schema_not_configured),
        )
        .route(
            "/api/collections/{id}",
            get(api::schema_not_configured)
                .patch(api::schema_not_configured)
                .delete(api::schema_not_configured),
        )
        .route(
            "/api/conversations",
            get(api::schema_not_configured).post(api::schema_not_configured),
        )
        .route(
            "/api/conversations/{id}",
            get(api::schema_not_configured).delete(api::schema_not_configured),
        )
        .route("/api/chat", axum::routing::post(api::schema_not_configured))
        .fallback(api::not_found)
        .method_not_allowed_fallback(api::method_not_allowed)
        .with_state(config);

    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            if let Err(error) = tokio::signal::ctrl_c().await {
                eprintln!("No se pudo registrar la señal de cierre: {error}");
            }
        })
        .await?;
    Ok(())
}

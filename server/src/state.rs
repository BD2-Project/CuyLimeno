use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, RwLock,
};

use axum::http::StatusCode;

use crate::{config::Config, db::Db, error::ApiError};

pub struct AppState {
    pub config: Config,
    pub db: Db,
    pub schema_ready: AtomicBool,
    pub index_ready: AtomicBool,
    pub active_model: RwLock<Option<String>>,
}

pub type SharedState = Arc<AppState>;

impl AppState {
    pub fn schema_ready(&self) -> bool {
        self.schema_ready.load(Ordering::Relaxed)
    }

    pub fn mark_schema_ready(&self) {
        self.schema_ready.store(true, Ordering::Relaxed);
    }

    /// Estado real del índice INVERTED (verificado con un SEARCH en bootstrap).
    pub fn index_ready(&self) -> bool {
        self.index_ready.load(Ordering::Relaxed)
    }

    pub fn mark_index_ready(&self) {
        self.index_ready.store(true, Ordering::Relaxed);
    }

    /// Modelo de generación activo (puede cambiarse desde la configuración).
    pub fn active_model(&self) -> Option<String> {
        self.active_model
            .read()
            .ok()
            .and_then(|model| model.clone())
    }

    pub fn set_active_model(&self, model: String) {
        if let Ok(mut guard) = self.active_model.write() {
            *guard = Some(model);
        }
    }

    pub fn require_schema(&self) -> Result<(), ApiError> {
        if self.schema_ready() {
            return Ok(());
        }
        Err(ApiError::new(
            StatusCode::NOT_IMPLEMENTED,
            "DOCUMENT_SCHEMA_NOT_CONFIGURED",
            "El esquema documental y la ingestión todavía no están configurados. \
             Arranca el motor SoupDB y vuelve a comprobar la conexión.",
        ))
    }
}

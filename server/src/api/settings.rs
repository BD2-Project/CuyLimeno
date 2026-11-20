use axum::{extract::State, http::StatusCode, Json};

use crate::{
    contracts::{ModelInfo, ModelUpdate},
    error::ApiError,
    llm::Groq,
    state::SharedState,
};

/// Modelos sugeridos para empezar (baratos/rápidos primero).
const PREFERRED: &[&str] = &[
    "openai/gpt-oss-20b",
    "openai/gpt-oss-120b",
    "qwen/qwen3.8-27b",
];

fn is_chat_model(id: &str) -> bool {
    let id = id.to_ascii_lowercase();
    !id.contains("whisper")
        && !id.contains("orpheus")
        && !id.contains("tts")
        && !id.contains("guard")
        && !id.contains("safeguard")
}

fn ordered(mut models: Vec<ModelInfo>) -> Vec<ModelInfo> {
    models.retain(|model| is_chat_model(&model.id));
    let rank = |id: &str| PREFERRED.iter().position(|name| *name == id).unwrap_or(PREFERRED.len());
    models.sort_by(|left, right| rank(&left.id).cmp(&rank(&right.id)).then(left.id.cmp(&right.id)));
    models
}

/// Lista los modelos de chat disponibles para la API key configurada.
pub async fn models(State(state): State<SharedState>) -> Result<Json<Vec<ModelInfo>>, ApiError> {
    let groq = Groq::from_config(&state.config).ok_or_else(|| {
        ApiError::service_unavailable(
            "GENERATION_UNAVAILABLE",
            "Falta GROQ_API_KEY en el servidor.",
        )
    })?;
    let models = groq.list_models().await.map_err(groq_error)?;
    Ok(Json(ordered(models)))
}

/// Cambia el modelo activo (se valida contra Groq y se persiste en SoupDB).
pub async fn set_model(
    State(state): State<SharedState>,
    Json(input): Json<ModelUpdate>,
) -> Result<Json<ModelInfo>, ApiError> {
    let requested = input.model.trim().to_owned();
    if requested.is_empty() {
        return Err(ApiError::bad_request(
            "INVALID_MODEL",
            "El modelo no puede estar vacío.",
        ));
    }
    let groq = Groq::from_config(&state.config).ok_or_else(|| {
        ApiError::service_unavailable(
            "GENERATION_UNAVAILABLE",
            "Falta GROQ_API_KEY en el servidor.",
        )
    })?;
    let available = groq.list_models().await.map_err(groq_error)?;
    let Some(model) = ordered(available)
        .into_iter()
        .find(|model| model.id == requested)
    else {
        return Err(ApiError::bad_request(
            "MODEL_NOT_FOUND",
            format!("El modelo '{requested}' no está disponible para esta API key."),
        ));
    };
    state.db.set_meta("active_model", &model.id).await?;
    state.set_active_model(model.id.clone());
    Ok(Json(model))
}

fn groq_error(error: crate::llm::GroqError) -> ApiError {
    ApiError::new(
        StatusCode::BAD_GATEWAY,
        "GROQ_ERROR",
        format!("No se pudo consultar GroqCloud: {error}"),
    )
}

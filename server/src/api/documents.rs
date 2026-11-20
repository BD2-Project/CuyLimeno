use std::{collections::HashMap, path::PathBuf};

use axum::{
    extract::{Multipart, Path, Query, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use uuid::Uuid;

use crate::{
    contracts::{Document, DocumentStatus},
    db::{as_int, as_text, sql_literal, DbError, Pooled},
    error::ApiError,
    ingest::{extraer_paginas, fragmentar, Fragmento},
    state::SharedState,
};

pub async fn list(State(state): State<SharedState>) -> Result<Json<Vec<Document>>, ApiError> {
    state.require_schema()?;
    let mut connection = state.db.acquire().await?;
    let documents = load_documents(&mut connection).await?;
    Ok(Json(documents))
}

pub async fn get(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Result<Json<Document>, ApiError> {
    state.require_schema()?;
    let mut connection = state.db.acquire().await?;
    let documents = load_documents(&mut connection).await?;
    documents
        .into_iter()
        .find(|document| document.id == id)
        .map(Json)
        .ok_or_else(|| ApiError::not_found("DOCUMENT_NOT_FOUND", "El documento no existe."))
}

#[derive(serde::Deserialize)]
pub struct ContentQuery {
    pub page: Option<i32>,
}

/// Texto extraído **por página** (evita cargar documentos grandes completos).
pub async fn content(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Query(query): Query<ContentQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    state.require_schema()?;
    let mut connection = state.db.acquire().await?;
    let document = connection
        .result(&format!(
            "SELECT page_count FROM cuy_documents WHERE id = {} LIMIT 1",
            sql_literal(&id)
        ))
        .await?;
    let Some(row) = document.rows.first() else {
        return Err(ApiError::not_found(
            "DOCUMENT_NOT_FOUND",
            "El documento no existe.",
        ));
    };
    let pages = row.first().and_then(as_int).unwrap_or(0).max(0);
    let page = query.page.unwrap_or(1).max(1);
    let chunks = connection
        .result(&format!(
            "SELECT pos, page, texto FROM cuy_chunks WHERE document_id = {} AND page = {} \
             ORDER BY pos",
            sql_literal(&id),
            page
        ))
        .await?;
    let mut blocks = Vec::with_capacity(chunks.rows.len());
    for row in &chunks.rows {
        let pos = row.first().and_then(as_int).unwrap_or(0);
        let chunk_page = row.get(1).and_then(as_int).unwrap_or(page).max(1);
        let texto = row.get(2).and_then(as_text).unwrap_or_default();
        blocks.push(serde_json::json!({
            "pos": pos,
            "page": chunk_page,
            "texto": texto,
        }));
    }
    Ok(Json(serde_json::json!({
        "id": id,
        "page": page,
        "pages": pages,
        "chunks": blocks,
    })))
}

/// Sirve el PDF original para verlo con el visor del navegador.
pub async fn file(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Result<impl axum::response::IntoResponse, ApiError> {
    state.require_schema()?;
    let mut connection = state.db.acquire().await?;
    let document = connection
        .result(&format!(
            "SELECT filename FROM cuy_documents WHERE id = {} LIMIT 1",
            sql_literal(&id)
        ))
        .await?;
    let Some(row) = document.rows.first() else {
        return Err(ApiError::not_found(
            "DOCUMENT_NOT_FOUND",
            "El documento no existe.",
        ));
    };
    let filename = row
        .first()
        .and_then(as_text)
        .unwrap_or_else(|| "documento.pdf".to_owned());
    drop(connection);
    let path = state.config.data_dir.join(format!("{id}.pdf"));
    let bytes = tokio::fs::read(&path).await.map_err(|error| {
        ApiError::not_found(
            "DOCUMENT_FILE_NOT_FOUND",
            format!("El PDF original no está disponible: {error}"),
        )
    })?;
    let safe_name = filename.replace(['"', '\n', '\r'], "");
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("application/pdf"),
    );
    headers.insert(
        axum::http::header::CONTENT_DISPOSITION,
        axum::http::HeaderValue::from_str(&format!("inline; filename=\"{safe_name}\""))
            .unwrap_or_else(|_| axum::http::HeaderValue::from_static("inline")),
    );
    Ok((headers, bytes))
}

pub async fn create(
    State(state): State<SharedState>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<Document>), ApiError> {
    state.require_schema()?;
    let mut bytes: Option<Vec<u8>> = None;
    let mut original_name: Option<String> = None;
    let mut title_field: Option<String> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|error| ApiError::bad_request("INVALID_MULTIPART", format!("{error}")))?
    {
        let field_name = field.name().unwrap_or_default().to_owned();
        match field_name.as_str() {
            "file" => {
                if let Some(file_name) = field.file_name() {
                    original_name = Some(file_name.to_owned());
                }
                let data = field
                    .bytes()
                    .await
                    .map_err(|error| ApiError::bad_request("INVALID_MULTIPART", format!("{error}")))?;
                bytes = Some(data.to_vec());
            }
            "title" => {
                title_field = Some(
                    field
                        .text()
                        .await
                        .map_err(|error| ApiError::bad_request("INVALID_MULTIPART", format!("{error}")))?,
                );
            }
            _ => {
                let _ = field.bytes().await;
            }
        }
    }

    let bytes = bytes.ok_or_else(|| {
        ApiError::bad_request("FILE_REQUIRED", "Falta el campo 'file' con el PDF.")
    })?;
    let filename = original_name.unwrap_or_else(|| "documento.pdf".to_owned());
    if !filename.to_lowercase().ends_with(".pdf") {
        return Err(ApiError::bad_request(
            "UNSUPPORTED_FILE_TYPE",
            "Solo se admiten archivos PDF.",
        ));
    }
    if bytes.is_empty() {
        return Err(ApiError::bad_request("EMPTY_FILE", "El archivo está vacío."));
    }
    if bytes.len() as u64 > state.config.max_upload_bytes {
        return Err(ApiError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "FILE_TOO_LARGE",
            format!(
                "El archivo supera el límite de {} MB.",
                state.config.max_upload_bytes / (1024 * 1024)
            ),
        ));
    }

    let id = Uuid::new_v4().to_string();
    let path = state.config.data_dir.join(format!("{id}.pdf"));
    tokio::fs::write(&path, &bytes)
        .await
        .map_err(|error| ApiError::internal("STORAGE_ERROR", format!("No se pudo guardar el PDF: {error}")))?;

    let title = title_field
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            std::path::Path::new(&filename)
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("Documento")
                .to_owned()
        });
    let now = Utc::now().to_rfc3339();

    let mut connection = state.db.acquire().await?;
    connection
        .execute(&format!(
            "INSERT INTO cuy_documents VALUES ({}, {}, '[]', NULL, {}, NULL, 'processing', {})",
            sql_literal(&id),
            sql_literal(&title),
            sql_literal(&filename),
            sql_literal(&now)
        ))
        .await?;
    drop(connection);

    let document = Document {
        id: id.clone(),
        title,
        authors: Vec::new(),
        year: None,
        filename,
        page_count: None,
        collection_ids: Vec::new(),
        created_at: now,
        status: DocumentStatus::Processing,
    };

    let task_state = state.clone();
    tokio::spawn(async move {
        process_document(task_state, id, path).await;
    });

    Ok((StatusCode::CREATED, Json(document)))
}

pub async fn delete(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    state.require_schema()?;
    {
        let mut connection = state.db.acquire().await?;
        let exists = connection
            .result(&format!(
                "SELECT id FROM cuy_documents WHERE id = {} LIMIT 1",
                sql_literal(&id)
            ))
            .await?
            .rows
            .first()
            .is_some();
        if !exists {
            return Err(ApiError::not_found(
                "DOCUMENT_NOT_FOUND",
                "El documento no existe.",
            ));
        }
        let statements = vec![
            format!(
                "DELETE FROM cuy_citations WHERE document_id = {}",
                sql_literal(&id)
            ),
            format!("DELETE FROM cuy_chunks WHERE document_id = {}", sql_literal(&id)),
            format!(
                "DELETE FROM cuy_collection_documents WHERE document_id = {}",
                sql_literal(&id)
            ),
            format!("DELETE FROM cuy_documents WHERE id = {}", sql_literal(&id)),
        ];
        match connection.transaction(&statements).await {
            Ok(()) => {}
            Err(error) if error.is_blocked() => {
                drop(connection);
                let mut connection = state.db.acquire().await?;
                drop_index(&mut connection).await?;
                connection
                    .execute(&format!(
                        "DELETE FROM cuy_chunks WHERE document_id = {}",
                        sql_literal(&id)
                    ))
                    .await?;
                recreate_index(&mut connection).await?;
                connection
                    .execute(&format!(
                        "DELETE FROM cuy_citations WHERE document_id = {}",
                        sql_literal(&id)
                    ))
                    .await?;
                connection
                    .execute(&format!(
                        "DELETE FROM cuy_collection_documents WHERE document_id = {}",
                        sql_literal(&id)
                    ))
                    .await?;
                connection
                    .execute(&format!(
                        "DELETE FROM cuy_documents WHERE id = {}",
                        sql_literal(&id)
                    ))
                    .await?;
            }
            Err(error) => return Err(error.into()),
        }
    }
    let _ = tokio::fs::remove_file(state.config.data_dir.join(format!("{id}.pdf"))).await;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn load_documents(
    connection: &mut Pooled<'_>,
) -> Result<Vec<Document>, DbError> {
    let documents = connection
        .result(
            "SELECT id, title, authors, year, filename, page_count, status, created_at \
             FROM cuy_documents ORDER BY created_at DESC",
        )
        .await?;
    let links = connection
        .result("SELECT collection_id, document_id FROM cuy_collection_documents")
        .await?;
    let mut collections: HashMap<String, Vec<String>> = HashMap::new();
    for row in &links.rows {
        if let (Some(collection_id), Some(document_id)) =
            (row.first().and_then(as_text), row.get(1).and_then(as_text))
        {
            collections
                .entry(document_id)
                .or_default()
                .push(collection_id);
        }
    }
    let mut result = Vec::new();
    for row in &documents.rows {
        let Some(id) = row.first().and_then(as_text) else {
            continue;
        };
        result.push(Document {
            title: row.get(1).and_then(as_text).unwrap_or_default(),
            authors: row
                .get(2)
                .and_then(as_text)
                .map(|raw| parse_authors(&raw))
                .unwrap_or_default(),
            year: row.get(3).and_then(as_int).map(|year| year.max(0) as u16),
            filename: row.get(4).and_then(as_text).unwrap_or_default(),
            page_count: row
                .get(5)
                .and_then(as_int)
                .map(|pages| pages.max(0) as u32),
            status: DocumentStatus::parse(
                &row.get(6).and_then(as_text).unwrap_or_default(),
            ),
            created_at: row.get(7).and_then(as_text).unwrap_or_default(),
            collection_ids: collections.get(&id).cloned().unwrap_or_default(),
            id,
        });
    }
    Ok(result)
}

pub(crate) fn parse_authors(raw: &str) -> Vec<String> {
    if raw.trim().is_empty() {
        return Vec::new();
    }
    serde_json::from_str::<Vec<String>>(raw).unwrap_or_else(|_| vec![raw.to_owned()])
}

async fn process_document(state: SharedState, id: String, path: PathBuf) {
    let result = tokio::task::spawn_blocking(move || {
        let bytes = std::fs::read(&path).map_err(|error| format!("no se pudo leer el PDF: {error}"))?;
        let pages = extraer_paginas(&bytes)?;
        let page_count = pages.len() as u32;
        let fragments = fragmentar(&pages);
        if fragments.is_empty() {
            return Err("el PDF no contiene texto extraíble".to_owned());
        }
        Ok((fragments, page_count))
    })
    .await;

    let (fragments, page_count) = match result {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => {
            eprintln!("CuyLimeno: ingestión fallida en {id}: {error}");
            mark_failed(&state, &id).await;
            return;
        }
        Err(error) => {
            eprintln!("CuyLimeno: ingestión abortada en {id}: {error}");
            mark_failed(&state, &id).await;
            return;
        }
    };

    if let Err(error) = write_chunks(&state, &id, &fragments).await {
        eprintln!("CuyLimeno: no se pudieron guardar los fragmentos de {id}: {error}");
        mark_failed(&state, &id).await;
        return;
    }
    if let Err(error) = mark_ready(&state, &id, page_count).await {
        eprintln!("CuyLimeno: no se pudo marcar {id} como listo: {error}");
    }
}

/// PDFs grandes generan cientos/miles de chunks: se insertan en lotes para no
/// construir una única transacción enorme.
const CHUNK_BATCH: usize = 200;

async fn write_chunks(
    state: &SharedState,
    id: &str,
    fragments: &[Fragmento],
) -> Result<(), DbError> {
    let statements: Vec<String> = fragments
        .iter()
        .map(|fragment| {
            format!(
                "INSERT INTO cuy_chunks VALUES ({}, {}, {}, {})",
                sql_literal(id),
                fragment.pos,
                fragment.page,
                sql_literal(&fragment.texto)
            )
        })
        .collect();
    let mut connection = state.db.acquire().await?;
    let mut start = 0;
    while start < statements.len() {
        let end = (start + CHUNK_BATCH).min(statements.len());
        match connection.transaction(&statements[start..end]).await {
            Ok(()) => start = end,
            // Respaldo cuando el motor rechaza el mantenimiento transaccional
            // del índice: reconstruir el índice alrededor de las inserciones.
            Err(error) if error.is_blocked() => {
                return rebuild_chunks(&mut connection, id, &statements).await;
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

async fn rebuild_chunks(
    connection: &mut Pooled<'_>,
    id: &str,
    statements: &[String],
) -> Result<(), DbError> {
    drop_index(connection).await?;
    connection
        .execute(&format!(
            "DELETE FROM cuy_chunks WHERE document_id = {}",
            sql_literal(id)
        ))
        .await?;
    for statement in statements {
        connection.execute(statement).await?;
    }
    recreate_index(connection).await
}

async fn drop_index(connection: &mut Pooled<'_>) -> Result<(), DbError> {
    match connection.execute("DROP INDEX idx_cuy_chunks").await {
        Ok(_) => Ok(()),
        Err(error) if error.message().contains("does not exist") => Ok(()),
        Err(error) => Err(error),
    }
}

async fn recreate_index(connection: &mut Pooled<'_>) -> Result<(), DbError> {
    match connection.execute(crate::db::INDEX_DDL).await {
        Ok(_) => Ok(()),
        Err(error) if error.is_already_exists() => Ok(()),
        Err(error) => Err(error),
    }
}

async fn mark_failed(state: &SharedState, id: &str) {
    if let Err(error) = set_document_state(state, id, "failed", None).await {
        eprintln!("CuyLimeno: no se pudo marcar {id} como fallido: {error}");
    }
}

async fn mark_ready(state: &SharedState, id: &str, page_count: u32) -> Result<(), DbError> {
    set_document_state(state, id, "ready", Some(page_count)).await
}

/// El motor parsea `UPDATE` pero no lo ejecuta: se reemplaza la fila completa
/// (DELETE + INSERT) dentro de una transacción.
async fn set_document_state(
    state: &SharedState,
    id: &str,
    status: &str,
    page_count: Option<u32>,
) -> Result<(), DbError> {
    let mut connection = state.db.acquire().await?;
    let current = connection
        .result(&format!(
            "SELECT title, authors, year, filename, created_at FROM cuy_documents \
             WHERE id = {} LIMIT 1",
            sql_literal(id)
        ))
        .await?;
    let Some(row) = current.rows.first() else {
        return Ok(());
    };
    let title = row.first().and_then(as_text).unwrap_or_default();
    let authors = row.get(1).and_then(as_text).unwrap_or_else(|| "[]".to_owned());
    let year = row
        .get(2)
        .and_then(as_int)
        .map(|value| value.to_string())
        .unwrap_or_else(|| "NULL".to_owned());
    let filename = row.get(3).and_then(as_text).unwrap_or_default();
    let created_at = row.get(4).and_then(as_text).unwrap_or_default();
    let page_count = page_count
        .map(|value| value.to_string())
        .unwrap_or_else(|| "NULL".to_owned());
    let insert = format!(
        "INSERT INTO cuy_documents VALUES ({}, {}, {}, {}, {}, {}, {}, {})",
        sql_literal(id),
        sql_literal(&title),
        sql_literal(&authors),
        year,
        sql_literal(&filename),
        page_count,
        sql_literal(status),
        sql_literal(&created_at)
    );
    connection
        .transaction(&[
            format!("DELETE FROM cuy_documents WHERE id = {}", sql_literal(id)),
            insert,
        ])
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn autores_json_y_texto_plano() {
        assert_eq!(
            parse_authors("[\"Ada\",\"Linus\"]"),
            vec!["Ada".to_owned(), "Linus".to_owned()]
        );
        assert_eq!(parse_authors("Ada Lovelace"), vec!["Ada Lovelace".to_owned()]);
        assert!(parse_authors("").is_empty());
    }
}

use std::collections::{HashMap, HashSet};

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use uuid::Uuid;

use crate::{
    contracts::{
        Collection, CollectionDetail, CollectionDocumentsInput, CollectionInput, CollectionUpdate,
    },
    db::{as_text, sql_literal, DbError, Pooled},
    error::ApiError,
    state::SharedState,
};

pub async fn list(State(state): State<SharedState>) -> Result<Json<Vec<Collection>>, ApiError> {
    state.require_schema()?;
    let mut connection = state.db.acquire().await?;
    Ok(Json(load_collections(&mut connection).await?))
}

pub async fn create(
    State(state): State<SharedState>,
    Json(input): Json<CollectionInput>,
) -> Result<(StatusCode, Json<Collection>), ApiError> {
    state.require_schema()?;
    let name = input.name.trim().to_owned();
    if name.is_empty() {
        return Err(ApiError::bad_request(
            "INVALID_COLLECTION_NAME",
            "El nombre de la colección no puede estar vacío.",
        ));
    }
    let id = Uuid::new_v4().to_string();
    let created_at = Utc::now().to_rfc3339();
    let mut connection = state.db.acquire().await?;
    connection
        .execute(&format!(
            "INSERT INTO cuy_collections VALUES ({}, {}, {}, {})",
            sql_literal(&id),
            sql_literal(&name),
            sql_literal(input.description.trim()),
            sql_literal(&created_at)
        ))
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(Collection {
            id,
            name,
            description: input.description.trim().to_owned(),
            document_count: 0,
            created_at,
        }),
    ))
}

pub async fn get(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Result<Json<CollectionDetail>, ApiError> {
    state.require_schema()?;
    let mut connection = state.db.acquire().await?;
    let collection = find_collection(&mut connection, &id).await?;
    let documents = super::documents::load_documents(&mut connection)
        .await?
        .into_iter()
        .filter(|document| document.collection_ids.iter().any(|value| value == &id))
        .collect();
    Ok(Json(CollectionDetail {
        collection,
        documents,
    }))
}

pub async fn update(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(input): Json<CollectionUpdate>,
) -> Result<Json<Collection>, ApiError> {
    state.require_schema()?;
    let mut connection = state.db.acquire().await?;
    let current = find_collection(&mut connection, &id).await?;
    let name = input
        .name
        .map(|value| value.trim().to_owned())
        .unwrap_or_else(|| current.name.clone());
    if name.is_empty() {
        return Err(ApiError::bad_request(
            "INVALID_COLLECTION_NAME",
            "El nombre de la colección no puede estar vacío.",
        ));
    }
    let description = input
        .description
        .unwrap_or_else(|| current.description.clone());
    connection
        .transaction(&[
            format!(
                "DELETE FROM cuy_collections WHERE id = {}",
                sql_literal(&id)
            ),
            format!(
                "INSERT INTO cuy_collections VALUES ({}, {}, {}, {})",
                sql_literal(&id),
                sql_literal(&name),
                sql_literal(&description),
                sql_literal(&current.created_at)
            ),
        ])
        .await?;
    Ok(Json(Collection {
        name,
        description,
        ..current
    }))
}

pub async fn delete(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    state.require_schema()?;
    let mut connection = state.db.acquire().await?;
    find_collection(&mut connection, &id).await?;
    connection
        .transaction(&[
            format!(
                "DELETE FROM cuy_collection_documents WHERE collection_id = {}",
                sql_literal(&id)
            ),
            format!("DELETE FROM cuy_collections WHERE id = {}", sql_literal(&id)),
        ])
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn add_documents(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(input): Json<CollectionDocumentsInput>,
) -> Result<StatusCode, ApiError> {
    state.require_schema()?;
    let mut connection = state.db.acquire().await?;
    find_collection(&mut connection, &id).await?;
    let links = connection
        .result(&format!(
            "SELECT document_id FROM cuy_collection_documents WHERE collection_id = {}",
            sql_literal(&id)
        ))
        .await?;
    let existing: HashSet<String> = links
        .rows
        .iter()
        .filter_map(|row| row.first().and_then(as_text))
        .collect();
    let statements: Vec<String> = input
        .document_ids
        .iter()
        .filter(|document_id| !existing.contains(*document_id))
        .map(|document_id| {
            format!(
                "INSERT INTO cuy_collection_documents VALUES ({}, {})",
                sql_literal(&id),
                sql_literal(document_id)
            )
        })
        .collect();
    connection.transaction(&statements).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn remove_document(
    State(state): State<SharedState>,
    Path((id, document_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    state.require_schema()?;
    let mut connection = state.db.acquire().await?;
    find_collection(&mut connection, &id).await?;
    connection
        .execute(&format!(
            "DELETE FROM cuy_collection_documents WHERE collection_id = {} AND document_id = {}",
            sql_literal(&id),
            sql_literal(&document_id)
        ))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn load_collections(
    connection: &mut Pooled<'_>,
) -> Result<Vec<Collection>, DbError> {
    let collections = connection
        .result("SELECT id, name, description, created_at FROM cuy_collections")
        .await?;
    let links = connection
        .result("SELECT collection_id FROM cuy_collection_documents")
        .await?;
    let mut counts: HashMap<String, u32> = HashMap::new();
    for row in &links.rows {
        if let Some(collection_id) = row.first().and_then(as_text) {
            *counts.entry(collection_id).or_insert(0) += 1;
        }
    }
    let mut result = Vec::new();
    for row in &collections.rows {
        let Some(id) = row.first().and_then(as_text) else {
            continue;
        };
        result.push(Collection {
            document_count: counts.get(&id).copied().unwrap_or(0),
            name: row.get(1).and_then(as_text).unwrap_or_default(),
            description: row.get(2).and_then(as_text).unwrap_or_default(),
            created_at: row.get(3).and_then(as_text).unwrap_or_default(),
            id,
        });
    }
    Ok(result)
}

async fn find_collection(
    connection: &mut Pooled<'_>,
    id: &str,
) -> Result<Collection, ApiError> {
    load_collections(connection)
        .await?
        .into_iter()
        .find(|collection| collection.id == id)
        .ok_or_else(|| ApiError::not_found("COLLECTION_NOT_FOUND", "La colección no existe."))
}

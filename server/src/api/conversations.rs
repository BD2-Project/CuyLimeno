use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use uuid::Uuid;

use crate::{
    contracts::{
        Citation, Conversation, ConversationInput, ConversationUpdate, Message, MessageRole,
    },
    db::{as_int, as_text, sql_literal, DbError, Pooled},
    error::ApiError,
    state::SharedState,
};

#[derive(Debug, serde::Deserialize)]
pub struct ConversationQuery {
    pub offset: Option<usize>,
    pub limit: Option<usize>,
}

pub async fn list(
    State(state): State<SharedState>,
    Query(query): Query<ConversationQuery>,
) -> Result<Json<Vec<Conversation>>, ApiError> {
    state.require_schema()?;
    let offset = query.offset.unwrap_or(0);
    let limit = query.limit.unwrap_or(20).clamp(1, 50);
    let mut connection = state.db.acquire().await?;
    Ok(Json(
        load_conversations_page(&mut connection, offset, limit).await?,
    ))
}

pub async fn create(
    State(state): State<SharedState>,
    Json(input): Json<ConversationInput>,
) -> Result<(StatusCode, Json<Conversation>), ApiError> {
    state.require_schema()?;
    let id = Uuid::new_v4().to_string();
    let updated_at = Utc::now().to_rfc3339();
    let title = input
        .title
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "Nueva conversación".to_owned());
    let mut connection = state.db.acquire().await?;
    connection
        .execute(&format!(
            "INSERT INTO cuy_conversations VALUES ({}, {}, {}, {}, {})",
            sql_literal(&id),
            sql_literal(&title),
            sql_literal(&json_list(&input.document_ids)),
            sql_literal(&json_list(&input.collection_ids)),
            sql_literal(&updated_at)
        ))
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(Conversation {
            id,
            title,
            document_ids: input.document_ids,
            collection_ids: input.collection_ids,
            updated_at,
            messages: Vec::new(),
        }),
    ))
}

pub async fn get(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Result<Json<Conversation>, ApiError> {
    state.require_schema()?;
    let mut connection = state.db.acquire().await?;
    let mut conversation = load_conversations(&mut connection)
        .await?
        .into_iter()
        .find(|conversation| conversation.id == id)
        .ok_or_else(|| {
            ApiError::not_found("CONVERSATION_NOT_FOUND", "La conversación no existe.")
        })?;
    conversation.messages = load_messages(&mut connection, &id).await?;
    Ok(Json(conversation))
}

pub async fn rename(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(input): Json<ConversationUpdate>,
) -> Result<Json<Conversation>, ApiError> {
    state.require_schema()?;
    let title = input.title.trim().to_owned();
    if title.is_empty() {
        return Err(ApiError::bad_request(
            "INVALID_CONVERSATION_TITLE",
            "El título de la conversación no puede estar vacío.",
        ));
    }
    if title.chars().count() > 120 {
        return Err(ApiError::bad_request(
            "INVALID_CONVERSATION_TITLE",
            "El título supera el límite de 120 caracteres.",
        ));
    }
    let mut connection = state.db.acquire().await?;
    let current = load_conversations(&mut connection)
        .await?
        .into_iter()
        .find(|conversation| conversation.id == id)
        .ok_or_else(|| {
            ApiError::not_found("CONVERSATION_NOT_FOUND", "La conversación no existe.")
        })?;
    connection
        .transaction(&[
            format!(
                "DELETE FROM cuy_conversations WHERE id = {}",
                sql_literal(&id)
            ),
            format!(
                "INSERT INTO cuy_conversations VALUES ({}, {}, {}, {}, {})",
                sql_literal(&id),
                sql_literal(&title),
                sql_literal(&json_list(&current.document_ids)),
                sql_literal(&json_list(&current.collection_ids)),
                sql_literal(&current.updated_at)
            ),
        ])
        .await?;
    Ok(Json(Conversation {
        title,
        ..current
    }))
}

pub async fn delete(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    state.require_schema()?;
    let mut connection = state.db.acquire().await?;
    let exists = load_conversations(&mut connection)
        .await?
        .iter()
        .any(|conversation| conversation.id == id);
    if !exists {
        return Err(ApiError::not_found(
            "CONVERSATION_NOT_FOUND",
            "La conversación no existe.",
        ));
    }
    let messages = load_messages(&mut connection, &id).await?;
    let mut statements = Vec::new();
    for message in &messages {
        statements.push(format!(
            "DELETE FROM cuy_citations WHERE message_id = {}",
            sql_literal(&message.id)
        ));
    }
    statements.push(format!(
        "DELETE FROM cuy_messages WHERE conversation_id = {}",
        sql_literal(&id)
    ));
    statements.push(format!(
        "DELETE FROM cuy_conversations WHERE id = {}",
        sql_literal(&id)
    ));
    connection.transaction(&statements).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn load_conversations(
    connection: &mut Pooled<'_>,
) -> Result<Vec<Conversation>, DbError> {
    let conversations = connection
        .result(
            "SELECT id, title, document_ids, collection_ids, updated_at \
             FROM cuy_conversations ORDER BY updated_at DESC",
        )
        .await?;
    Ok(conversations_from_rows(&conversations.rows))
}

pub(crate) async fn load_conversations_page(
    connection: &mut Pooled<'_>,
    offset: usize,
    limit: usize,
) -> Result<Vec<Conversation>, DbError> {
    let conversations = connection
        .result(&format!(
            "SELECT id, title, document_ids, collection_ids, updated_at \
             FROM cuy_conversations ORDER BY updated_at DESC LIMIT {} OFFSET {}",
            limit, offset
        ))
        .await?;
    Ok(conversations_from_rows(&conversations.rows))
}

fn conversations_from_rows(rows: &[Vec<rsoup::Value>]) -> Vec<Conversation> {
    let mut result = Vec::new();
    for row in rows {
        let Some(id) = row.first().and_then(as_text) else {
            continue;
        };
        result.push(Conversation {
            title: row.get(1).and_then(as_text).unwrap_or_default(),
            document_ids: row.get(2).and_then(as_text).map(|raw| parse_list(&raw)).unwrap_or_default(),
            collection_ids: row.get(3).and_then(as_text).map(|raw| parse_list(&raw)).unwrap_or_default(),
            updated_at: row.get(4).and_then(as_text).unwrap_or_default(),
            messages: Vec::new(),
            id,
        });
    }
    result
}

async fn load_messages(
    connection: &mut Pooled<'_>,
    conversation_id: &str,
) -> Result<Vec<Message>, DbError> {
    let messages = connection
        .result(&format!(
            "SELECT id, role, content, created_at FROM cuy_messages \
             WHERE conversation_id = {} ORDER BY created_at",
            sql_literal(conversation_id)
        ))
        .await?;
    let mut result = Vec::new();
    for row in &messages.rows {
        let Some(id) = row.first().and_then(as_text) else {
            continue;
        };
        let citations = connection
            .result(&format!(
                "SELECT document_id, title, page, excerpt FROM cuy_citations \
                 WHERE message_id = {}",
                sql_literal(&id)
            ))
            .await?;
        let citations = citations
            .rows
            .iter()
            .filter_map(|row| {
                Some(Citation {
                    document_id: row.first().and_then(as_text)?,
                    document_title: row.get(1).and_then(as_text).unwrap_or_default(),
                    page: row.get(2).and_then(as_int).unwrap_or(1).max(1) as u32,
                    excerpt: row.get(3).and_then(as_text).unwrap_or_default(),
                })
            })
            .collect();
        result.push(Message {
            role: MessageRole::parse(&row.get(1).and_then(as_text).unwrap_or_default()),
            content: row.get(2).and_then(as_text).unwrap_or_default(),
            created_at: row.get(3).and_then(as_text).unwrap_or_default(),
            citations,
            id,
        });
    }
    Ok(result)
}

pub(crate) fn json_list(values: &[String]) -> String {
    serde_json::to_string(values).unwrap_or_else(|_| "[]".to_owned())
}

pub(crate) fn parse_list(raw: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(raw).unwrap_or_default()
}

/// El motor no ejecuta `UPDATE`: se reemplaza la fila de la conversación
/// (DELETE + INSERT) conservando sus campos y actualizando `updated_at`.
pub(crate) async fn touch_conversation(
    connection: &mut Pooled<'_>,
    conversation_id: &str,
    updated_at: &str,
) -> Result<(), DbError> {
    let current = connection
        .result(&format!(
            "SELECT title, document_ids, collection_ids FROM cuy_conversations \
             WHERE id = {} LIMIT 1",
            sql_literal(conversation_id)
        ))
        .await?;
    let Some(row) = current.rows.first() else {
        return Ok(());
    };
    let title = row.first().and_then(as_text).unwrap_or_default();
    let document_ids = row
        .get(1)
        .and_then(as_text)
        .unwrap_or_else(|| "[]".to_owned());
    let collection_ids = row
        .get(2)
        .and_then(as_text)
        .unwrap_or_else(|| "[]".to_owned());
    connection
        .transaction(&[
            format!(
                "DELETE FROM cuy_conversations WHERE id = {}",
                sql_literal(conversation_id)
            ),
            format!(
                "INSERT INTO cuy_conversations VALUES ({}, {}, {}, {}, {})",
                sql_literal(conversation_id),
                sql_literal(&title),
                sql_literal(&document_ids),
                sql_literal(&collection_ids),
                sql_literal(updated_at)
            ),
        ])
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lista_json_ida_y_vuelta() {
        let values = vec!["uno".to_owned(), "dos".to_owned()];
        assert_eq!(json_list(&values), r#"["uno","dos"]"#);
        assert_eq!(parse_list(r#"["uno","dos"]"#), values);
        assert!(parse_list("no-json").is_empty());
    }
}

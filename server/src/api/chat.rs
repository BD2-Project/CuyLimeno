use std::{collections::{HashMap, HashSet}, convert::Infallible};

use axum::{
    extract::State,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    Json,
};
use chrono::Utc;
use futures_util::stream;
use tokio::sync::mpsc::{self, Sender};
use uuid::Uuid;

use crate::{
    contracts::{
        ChatCitationsEvent, ChatDeltaEvent, ChatDoneEvent, ChatErrorEvent, ChatInput,
        ChatMetaEvent, Citation,
    },
    db::{as_text, sql_literal, DbError, Pooled},
    error::ApiError,
    ingest::recortar,
    llm::{ChatMessage, Groq, GroqEvent},
    retrieval::{citas, recuperar, titulos, FragmentoRecuperado},
    state::SharedState,
};

const GROUNDED_PROMPT: &str = "Eres CuyLimeño, asistente de investigación académica. \
Prioriza siempre la información de los fragmentos proporcionados y cita con [n] cada afirmación que provenga de ellos. \
Si los fragmentos no responden a la pregunta pero es una pregunta general (una definición o un concepto), \
responde con una explicación general clara y completa; puedes usar listas o una tabla si ayudan a organizar la información. \
Termina esa respuesta con una nota breve: «Esta explicación se basa en conocimiento general; no está respaldada por los documentos proporcionados.» \
No uses marcadores [n] en esa parte. \
Si la pregunta requiere datos concretos de los documentos y no están en los fragmentos, dilo con claridad. \
Nunca inventes citas ni atribuyas afirmaciones a documentos que no has recibido.";

const CONVERSATIONAL_PROMPT: &str = "Eres CuyLimeño, un asistente académico conversacional. \
No se encontraron fragmentos de los documentos del usuario que cubran esta pregunta \
(biblioteca vacía o sin coincidencias). Responde de forma útil y natural con tu conocimiento, \
pero dilo con honestidad: aclara que esta respuesta no está respaldada por sus documentos y \
sugiere añadir fuentes o reformular la pregunta si necesita evidencia. \
No inventes citas ni atribuyas afirmaciones a documentos que no has recibido.";

const HISTORY_LIMIT: usize = 6;
const CONTEXT_WORDS: usize = 200;
const MAX_MESSAGE_CHARS: usize = 12_000;

pub async fn chat(
    State(state): State<SharedState>,
    Json(input): Json<ChatInput>,
) -> Result<Response, ApiError> {
    state.require_schema()?;
    let message = input.message.trim().to_owned();
    if message.is_empty() {
        return Err(ApiError::bad_request(
            "EMPTY_MESSAGE",
            "La pregunta no puede estar vacía.",
        ));
    }
    if message.chars().count() > MAX_MESSAGE_CHARS {
        return Err(ApiError::bad_request(
            "MESSAGE_TOO_LONG",
            format!("La pregunta supera el límite de {MAX_MESSAGE_CHARS} caracteres."),
        ));
    }
    let Some(model) = state.active_model() else {
        return Ok(sse_error(
            "GENERATION_UNAVAILABLE",
            "No hay modelo de generación configurado. Elige uno en Configuración.",
        ));
    };
    let Some(groq) = Groq::from_config(&state.config).map(|client| client.with_model(model)) else {
        return Ok(sse_error(
            "GENERATION_UNAVAILABLE",
            "GroqCloud no está configurada en el servidor (GROQ_API_KEY).",
        ));
    };

    let mut connection = state.db.acquire().await?;
    let conversation_id = match input.conversation_id.clone() {
        Some(id) => {
            if !conversation_exists(&mut connection, &id).await? {
                return Err(ApiError::not_found(
                    "CONVERSATION_NOT_FOUND",
                    "La conversación no existe.",
                ));
            }
            id
        }
        None => {
            let id = Uuid::new_v4().to_string();
            let now = Utc::now().to_rfc3339();
            connection
                .execute(&format!(
                    "INSERT INTO cuy_conversations VALUES ({}, {}, {}, {}, {})",
                    sql_literal(&id),
                    sql_literal(&titulo_desde(&message)),
                    sql_literal(&json_list(&input.document_ids)),
                    sql_literal(&json_list(&input.collection_ids)),
                    sql_literal(&now)
                ))
                .await?;
            id
        }
    };
    let history = last_messages(&mut connection, &conversation_id, HISTORY_LIMIT).await?;
    let user_message_id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    connection
        .execute(&format!(
            "INSERT INTO cuy_messages VALUES ({}, {}, 'user', {}, {})",
            sql_literal(&user_message_id),
            sql_literal(&conversation_id),
            sql_literal(&message),
            sql_literal(&now)
        ))
        .await?;
    let scope = resolve_scope(&mut connection, &input).await?;
    drop(connection);

    let fragments = recuperar(&state.db, &message, &scope).await?;
    let titles = titulos(&state.db).await?;
    let assistant_message_id = Uuid::new_v4().to_string();

    let (sender, receiver) = mpsc::channel::<Result<Event, Infallible>>(64);
    let meta = ChatMetaEvent {
        conversation_id: conversation_id.clone(),
        user_message_id,
        assistant_message_id: assistant_message_id.clone(),
    };
    let _ = sender.send(Ok(event("meta", &meta))).await;

    let task_state = state.clone();
    let task_conversation = conversation_id.clone();
    let task_assistant = assistant_message_id.clone();
    tokio::spawn(async move {
        run_generation(
            task_state,
            groq,
            sender,
            task_conversation,
            task_assistant,
            message,
            history,
            fragments,
            titles,
        )
        .await;
    });

    let stream = stream::unfold(receiver, |mut receiver| async move {
        receiver.recv().await.map(|item| (item, receiver))
    });
    Ok(Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response())
}

#[allow(clippy::too_many_arguments)]
async fn run_generation(
    state: SharedState,
    groq: Groq,
    sender: Sender<Result<Event, Infallible>>,
    conversation_id: String,
    assistant_message_id: String,
    message: String,
    history: Vec<ChatMessage>,
    fragments: Vec<FragmentoRecuperado>,
    titles: HashMap<String, String>,
) {
    let has_sources = !fragments.is_empty();
    let system = if has_sources {
        GROUNDED_PROMPT
    } else {
        CONVERSATIONAL_PROMPT
    };
    let user_content = build_user_content(&message, &fragments, &titles);
    let mut messages = history;
    messages.push(ChatMessage {
        role: "user".to_owned(),
        content: user_content,
    });

    let mut receiver = groq.spawn_stream(system, messages);
    let mut answer = String::new();
    let mut usage = None;
    while let Some(item) = receiver.recv().await {
        match item {
            Ok(GroqEvent::Delta(content)) => {
                answer.push_str(&content);
                let delta = ChatDeltaEvent { content };
                if sender.send(Ok(event("delta", &delta))).await.is_err() {
                    return;
                }
            }
            Ok(GroqEvent::Done(event_usage)) => usage = event_usage,
            Err(error) => {
                eprintln!("CuyLimeno: error de generación: {error}");
                let _ = sender
                    .send(Ok(event(
                        "error",
                        &ChatErrorEvent {
                            code: "GROQ_ERROR".to_owned(),
                            message: format!("{error}"),
                        },
                    )))
                    .await;
                return;
            }
        }
    }

    let citations = citas(&fragments, &titles, &answer);
    let _ = sender
        .send(Ok(event(
            "citations",
            &ChatCitationsEvent {
                citations: citations.clone(),
            },
        )))
        .await;
    let _ = sender
        .send(Ok(event(
            "done",
            &ChatDoneEvent {
                message_id: assistant_message_id.clone(),
                usage,
            },
        )))
        .await;
    persist_assistant(
        &state,
        &conversation_id,
        &assistant_message_id,
        answer,
        &citations,
    )
    .await;
}

fn build_user_content(
    message: &str,
    fragments: &[FragmentoRecuperado],
    titles: &HashMap<String, String>,
) -> String {
    if fragments.is_empty() {
        return message.to_owned();
    }
    let context = fragments
        .iter()
        .enumerate()
        .map(|(index, fragment)| {
            format!(
                "[{}] ({}, p. {})\n«{}»",
                index + 1,
                titles
                    .get(&fragment.document_id)
                    .map(String::as_str)
                    .unwrap_or("Documento"),
                fragment.page,
                recortar(&fragment.texto, CONTEXT_WORDS)
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    format!("Pregunta: {message}\n\nFragmentos disponibles:\n{context}")
}

async fn persist_assistant(
    state: &SharedState,
    conversation_id: &str,
    message_id: &str,
    content: String,
    citations: &[Citation],
) {
    let Ok(mut connection) = state.db.acquire().await else {
        return;
    };
    let now = Utc::now().to_rfc3339();
    let mut statements = vec![format!(
        "INSERT INTO cuy_messages VALUES ({}, {}, 'assistant', {}, {})",
        sql_literal(message_id),
        sql_literal(conversation_id),
        sql_literal(&content),
        sql_literal(&now)
    )];
    for citation in citations {
        statements.push(format!(
            "INSERT INTO cuy_citations VALUES ({}, {}, {}, {}, {})",
            sql_literal(message_id),
            sql_literal(&citation.document_id),
            sql_literal(&citation.document_title),
            citation.page,
            sql_literal(&citation.excerpt)
        ));
    }
    if let Err(error) = connection.transaction(&statements).await {
        eprintln!("CuyLimeno: no se pudo guardar la respuesta: {error}");
        return;
    }
    if let Err(error) =
        super::conversations::touch_conversation(&mut connection, conversation_id, &now).await
    {
        eprintln!("CuyLimeno: no se pudo actualizar la conversación: {error}");
    }
}

async fn conversation_exists(
    connection: &mut Pooled<'_>,
    id: &str,
) -> Result<bool, DbError> {
    Ok(connection
        .result(&format!(
            "SELECT id FROM cuy_conversations WHERE id = {} LIMIT 1",
            sql_literal(id)
        ))
        .await?
        .rows
        .first()
        .is_some())
}

async fn last_messages(
    connection: &mut Pooled<'_>,
    conversation_id: &str,
    limit: usize,
) -> Result<Vec<ChatMessage>, DbError> {
    let result = connection
        .result(&format!(
            "SELECT role, content FROM cuy_messages WHERE conversation_id = {} \
             ORDER BY created_at",
            sql_literal(conversation_id)
        ))
        .await?;
    let mut messages: Vec<ChatMessage> = result
        .rows
        .iter()
        .filter_map(|row| {
            Some(ChatMessage {
                role: row.first().and_then(as_text)?,
                content: row.get(1).and_then(as_text)?,
            })
        })
        .collect();
    if messages.len() > limit {
        messages = messages.split_off(messages.len() - limit);
    }
    Ok(messages)
}

async fn resolve_scope(
    connection: &mut Pooled<'_>,
    input: &ChatInput,
) -> Result<Option<HashSet<String>>, DbError> {
    let mut ids: HashSet<String> = input.document_ids.iter().cloned().collect();
    for collection_id in &input.collection_ids {
        let result = connection
            .result(&format!(
                "SELECT document_id FROM cuy_collection_documents WHERE collection_id = {}",
                sql_literal(collection_id)
            ))
            .await?;
        for row in &result.rows {
            if let Some(document_id) = row.first().and_then(as_text) {
                ids.insert(document_id);
            }
        }
    }
    if ids.is_empty() {
        Ok(None)
    } else {
        Ok(Some(ids))
    }
}

fn titulo_desde(message: &str) -> String {
    let mut title: String = message.chars().take(60).collect();
    if message.chars().count() > 60 {
        title.push('…');
    }
    title
}

fn json_list(values: &[String]) -> String {
    super::conversations::json_list(values)
}

fn event<T: serde::Serialize>(name: &str, payload: &T) -> Event {
    Event::default()
        .event(name)
        .data(serde_json::to_string(payload).unwrap_or_else(|_| "{}".to_owned()))
}

fn sse_error(code: &str, message: &str) -> Response {
    let body = serde_json::json!({ "code": code, "message": message }).to_string();
    let event = Event::default().event("error").data(body);
    let stream = stream::iter(vec![Ok::<Event, Infallible>(event)]);
    Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titulo_desde_recorta_y_agrega_elipsis() {
        assert_eq!(titulo_desde("pregunta corta"), "pregunta corta");
        let largo = "a".repeat(80);
        let titulo = titulo_desde(&largo);
        assert_eq!(titulo.chars().count(), 61);
        assert!(titulo.ends_with('…'));
    }

    #[test]
    fn sin_fragmentos_el_prompt_es_solo_la_pregunta() {
        let content = build_user_content("¿De qué tratan?", &[], &HashMap::new());
        assert_eq!(content, "¿De qué tratan?");
    }

    #[test]
    fn con_fragmentos_se_numeran_y_etiquetan_para_citar() {
        let fragments = vec![FragmentoRecuperado {
            document_id: "doc-1".to_owned(),
            pos: 1,
            page: 3,
            texto: "contenido verificable".to_owned(),
            score: 1.0,
        }];
        let mut titles = HashMap::new();
        titles.insert("doc-1".to_owned(), "Paper".to_owned());
        let content = build_user_content("¿Qué dice?", &fragments, &titles);
        assert!(content.contains("Pregunta: ¿Qué dice?"));
        assert!(content.contains("[1] (Paper, p. 3)"));
        assert!(content.contains("contenido verificable"));
    }
}

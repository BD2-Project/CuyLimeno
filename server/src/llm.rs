//! Cliente mínimo de GroqCloud con streaming SSE.

use std::time::Duration;

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::{config::Config, contracts::{ModelInfo, TokenUsage}};

#[derive(Debug, Clone)]
pub enum GroqError {
    Unavailable(String),
    Http { status: u16, body: String },
    Protocol(String),
}

impl std::fmt::Display for GroqError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GroqError::Unavailable(message) => write!(formatter, "Groq no disponible: {message}"),
            GroqError::Http { status, body } => {
                write!(formatter, "Groq respondió HTTP {status}: {body}")
            }
            GroqError::Protocol(message) => write!(formatter, "respuesta SSE inválida: {message}"),
        }
    }
}

#[derive(Debug, Clone)]
pub enum GroqEvent {
    Delta(String),
    Done(Option<TokenUsage>),
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Clone)]
pub struct Groq {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    model: String,
    timeout: Duration,
}

impl Groq {
    pub fn from_config(config: &Config) -> Option<Self> {
        let api_key = config.groq_api_key.clone()?;
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .build()
            .ok()?;
        Some(Self {
            client,
            base_url: config.groq_base_url.clone(),
            api_key,
            model: config.groq_model.clone().unwrap_or_default(),
            timeout: config.groq_timeout,
        })
    }

    pub fn with_model(mut self, model: String) -> Self {
        self.model = model;
        self
    }

    /// Lista los modelos visibles para la API key (sin exponerla al cliente).
    pub async fn list_models(&self) -> Result<Vec<ModelInfo>, GroqError> {
        #[derive(Deserialize)]
        struct ModelsResponse {
            #[serde(default)]
            data: Vec<ModelEntry>,
        }
        #[derive(Deserialize)]
        struct ModelEntry {
            id: String,
            #[serde(default)]
            context_window: Option<u32>,
            #[serde(default)]
            owned_by: Option<String>,
        }
        let response = self
            .client
            .get(format!("{}/models", self.base_url))
            .bearer_auth(&self.api_key)
            .timeout(self.timeout)
            .send()
            .await
            .map_err(|error| GroqError::Unavailable(error.to_string()))?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(GroqError::Http {
                status: status.as_u16(),
                body: truncar(&body, 300),
            });
        }
        let payload: ModelsResponse = response
            .json()
            .await
            .map_err(|error| GroqError::Protocol(error.to_string()))?;
        Ok(payload
            .data
            .into_iter()
            .map(|entry| ModelInfo {
                id: entry.id,
                context_window: entry.context_window,
                owned_by: entry.owned_by,
            })
            .collect())
    }

    /// Lanza la petición en una tarea y devuelve un canal con los deltas.
    pub fn spawn_stream(
        &self,
        system: &str,
        messages: Vec<ChatMessage>,
    ) -> mpsc::Receiver<Result<GroqEvent, GroqError>> {
        let (sender, receiver) = mpsc::channel(64);
        let client = self.client.clone();
        let url = format!("{}/chat/completions", self.base_url);
        let api_key = self.api_key.clone();
        let model = self.model.clone();
        let timeout = self.timeout;
        let system = system.to_owned();
        tokio::spawn(async move {
            run_stream(client, url, api_key, model, timeout, system, messages, sender).await;
        });
        receiver
    }
}

async fn run_stream(
    client: reqwest::Client,
    url: String,
    api_key: String,
    model: String,
    timeout: Duration,
    system: String,
    messages: Vec<ChatMessage>,
    sender: mpsc::Sender<Result<GroqEvent, GroqError>>,
) {
    let mut chat_messages = Vec::with_capacity(messages.len() + 1);
    chat_messages.push(ChatMessage {
        role: "system".to_owned(),
        content: system,
    });
    chat_messages.extend(messages);
    let body = serde_json::json!({
        "model": model,
        "messages": chat_messages,
        "stream": true,
        "temperature": 0.3,
        "max_completion_tokens": 1024,
    });
    let response = match client
        .post(&url)
        .bearer_auth(&api_key)
        .timeout(timeout)
        .json(&body)
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            let _ = sender
                .send(Err(GroqError::Unavailable(error.to_string())))
                .await;
            return;
        }
    };
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        let _ = sender
            .send(Err(GroqError::Http {
                status: status.as_u16(),
                body: truncar(&body, 500),
            }))
            .await;
        return;
    }

    let mut stream = response.bytes_stream();
    let mut buffer: Vec<u8> = Vec::new();
    let mut usage: Option<TokenUsage> = None;
    let mut finished = false;
    while let Some(chunk) = stream.next().await {
        let bytes = match chunk {
            Ok(bytes) => bytes,
            Err(error) => {
                let _ = sender
                    .send(Err(GroqError::Unavailable(error.to_string())))
                    .await;
                return;
            }
        };
        buffer.extend_from_slice(&bytes);
        while let Some(position) = buffer.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = buffer.drain(..=position).collect();
            let line = String::from_utf8_lossy(&line[..line.len() - 1]);
            let line = line.trim_end_matches('\r');
            let Some(data) = line.strip_prefix("data:") else {
                continue;
            };
            let data = data.trim();
            if data.is_empty() {
                continue;
            }
            if data == "[DONE]" {
                finished = true;
                break;
            }
            match parse_data_line(data) {
                Ok(Some(GroqEvent::Delta(content))) => {
                    if sender
                        .send(Ok(GroqEvent::Delta(content)))
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
                Ok(Some(GroqEvent::Done(event_usage))) => {
                    if event_usage.is_some() {
                        usage = event_usage;
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    let _ = sender.send(Err(error)).await;
                    return;
                }
            }
        }
        if finished {
            break;
        }
    }
    let _ = sender.send(Ok(GroqEvent::Done(usage))).await;
}

#[derive(Debug, Deserialize)]
struct StreamChunk {
    #[serde(default)]
    choices: Vec<StreamChoice>,
    #[serde(default)]
    usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
struct StreamChoice {
    #[serde(default)]
    delta: Option<Delta>,
}

#[derive(Debug, Deserialize)]
struct Delta {
    #[serde(default)]
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Usage {
    prompt_tokens: Option<u32>,
    completion_tokens: Option<u32>,
}

/// Parsea una línea `data:` del SSE de Groq (sin el prefijo `data:`).
pub fn parse_data_line(data: &str) -> Result<Option<GroqEvent>, GroqError> {
    let chunk: StreamChunk =
        serde_json::from_str(data).map_err(|error| GroqError::Protocol(error.to_string()))?;
    if let Some(usage) = chunk.usage {
        if let (Some(prompt_tokens), Some(completion_tokens)) =
            (usage.prompt_tokens, usage.completion_tokens)
        {
            return Ok(Some(GroqEvent::Done(Some(TokenUsage {
                prompt_tokens,
                completion_tokens,
            }))));
        }
    }
    if let Some(content) = chunk
        .choices
        .first()
        .and_then(|choice| choice.delta.as_ref())
        .and_then(|delta| delta.content.clone())
    {
        if !content.is_empty() {
            return Ok(Some(GroqEvent::Delta(content)));
        }
    }
    Ok(None)
}

fn truncar(texto: &str, max: usize) -> String {
    if texto.chars().count() <= max {
        return texto.to_owned();
    }
    let mut recorte: String = texto.chars().take(max).collect();
    recorte.push('…');
    recorte
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsea_delta_de_texto() {
        let line = r#"{"choices":[{"index":0,"delta":{"content":"Hola"}}]}"#;
        match parse_data_line(line).unwrap() {
            Some(GroqEvent::Delta(content)) => assert_eq!(content, "Hola"),
            other => panic!("evento inesperado: {other:?}"),
        }
    }

    #[test]
    fn ignora_deltas_vacios_y_roles() {
        let line = r#"{"choices":[{"index":0,"delta":{"role":"assistant"}}]}"#;
        assert!(parse_data_line(line).unwrap().is_none());
    }

    #[test]
    fn parsea_usage_final() {
        let line = r#"{"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":4}}"#;
        match parse_data_line(line).unwrap() {
            Some(GroqEvent::Done(Some(usage))) => {
                assert_eq!(usage.prompt_tokens, 10);
                assert_eq!(usage.completion_tokens, 4);
            }
            other => panic!("evento inesperado: {other:?}"),
        }
    }

    #[test]
    fn rechaza_json_invalido() {
        assert!(matches!(
            parse_data_line("no-es-json"),
            Err(GroqError::Protocol(_))
        ));
    }
}

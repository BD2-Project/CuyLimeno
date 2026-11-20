//! HTTP DTOs shared with the Svelte frontend.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub id: String,
    pub title: String,
    pub authors: Vec<String>,
    pub year: Option<u16>,
    pub filename: String,
    pub page_count: Option<u32>,
    pub collection_ids: Vec<String>,
    pub created_at: String,
    pub status: DocumentStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentStatus {
    Pending,
    Processing,
    Ready,
    Failed,
}

impl DocumentStatus {
    pub fn parse(value: &str) -> Self {
        match value {
            "processing" => DocumentStatus::Processing,
            "ready" => DocumentStatus::Ready,
            "failed" => DocumentStatus::Failed,
            _ => DocumentStatus::Pending,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Collection {
    pub id: String,
    pub name: String,
    pub description: String,
    pub document_count: u32,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
pub struct CollectionInput {
    pub name: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Deserialize)]
pub struct CollectionUpdate {
    pub name: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CollectionDocumentsInput {
    #[serde(default)]
    pub document_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct CollectionDetail {
    #[serde(flatten)]
    pub collection: Collection,
    pub documents: Vec<Document>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Citation {
    pub document_id: String,
    pub document_title: String,
    pub page: u32,
    pub excerpt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    User,
    Assistant,
}

impl MessageRole {
    pub fn parse(value: &str) -> Self {
        match value {
            "assistant" => MessageRole::Assistant,
            _ => MessageRole::User,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub role: MessageRole,
    pub content: String,
    pub citations: Vec<Citation>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    pub title: String,
    pub document_ids: Vec<String>,
    pub collection_ids: Vec<String>,
    pub updated_at: String,
    pub messages: Vec<Message>,
}

#[derive(Debug, Deserialize)]
pub struct ConversationInput {
    pub title: Option<String>,
    #[serde(default)]
    pub document_ids: Vec<String>,
    #[serde(default)]
    pub collection_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct ConversationUpdate {
    pub title: String,
}

#[derive(Debug, Deserialize)]
pub struct ChatInput {
    pub conversation_id: Option<String>,
    pub message: String,
    #[serde(default)]
    pub document_ids: Vec<String>,
    #[serde(default)]
    pub collection_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
}

#[derive(Debug, Serialize)]
pub struct ChatMetaEvent {
    pub conversation_id: String,
    pub user_message_id: String,
    pub assistant_message_id: String,
}

#[derive(Debug, Serialize)]
pub struct ChatDeltaEvent {
    pub content: String,
}

#[derive(Debug, Serialize)]
pub struct ChatCitationsEvent {
    pub citations: Vec<Citation>,
}

#[derive(Debug, Serialize)]
pub struct ChatDoneEvent {
    pub message_id: String,
    pub usage: Option<TokenUsage>,
}

#[derive(Debug, Serialize)]
pub struct ChatErrorEvent {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    #[serde(default)]
    pub context_window: Option<u32>,
    #[serde(default)]
    pub owned_by: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ModelUpdate {
    pub model: String,
}

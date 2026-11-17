//! Proposed HTTP DTOs, independent of the pending SoupDB document schema.
//! None of these types imply persisted records or a configured ingestion pipeline.
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
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

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentStatus {
    Pending,
    Processing,
    Ready,
    Failed,
}

#[derive(Debug, Serialize, Deserialize)]
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
    pub description: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Citation {
    pub document_id: String,
    pub document_title: String,
    pub page: u32,
    pub excerpt: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    User,
    Assistant,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub role: MessageRole,
    pub content: String,
    pub citations: Vec<Citation>,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    pub title: String,
    pub document_ids: Vec<String>,
    pub collection_ids: Vec<String>,
    pub updated_at: String,
    pub messages: Vec<Message>,
}

#[derive(Debug, Deserialize)]
pub struct ChatInput {
    pub conversation_id: Option<String>,
    pub message: String,
    pub document_ids: Vec<String>,
    pub collection_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ChatResponse {
    pub conversation_id: String,
    pub message: Message,
}

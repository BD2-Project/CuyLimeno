export type View = 'chat' | 'documents' | 'settings';
export const views: View[] = ['chat', 'documents', 'settings'];
export function viewFromHash(hash: string): View {
  const value = hash.replace(/^#\/?/, '');
  if (value === 'documents' || value === 'library') return 'documents';
  if (value === 'settings') return 'settings';
  return 'chat';
}

export interface Capabilities {
  driver: string;
  schema_configured: boolean;
  ingestion_available: boolean;
  retrieval_available: boolean;
  generation_available: boolean;
  groq_configured: boolean;
  groq_model: string | null;
}

export type DocumentStatus = 'pending' | 'processing' | 'ready' | 'failed';

export interface Document {
  id: string;
  title: string;
  authors: string[];
  year: number | null;
  filename: string;
  page_count: number | null;
  collection_ids: string[];
  created_at: string;
  status: DocumentStatus;
}

export interface Collection {
  id: string;
  name: string;
  description: string;
  document_count: number;
  created_at: string;
}

export interface Citation {
  document_id: string;
  document_title: string;
  page: number;
  excerpt: string;
}

export interface DocumentChunk {
  pos: number;
  page: number;
  texto: string;
}

export interface ModelInfo {
  id: string;
  context_window?: number | null;
  owned_by?: string | null;
}

export interface Message {
  id: string;
  role: 'user' | 'assistant';
  content: string;
  citations: Citation[];
  created_at: string;
}

export interface Conversation {
  id: string;
  title: string;
  document_ids: string[];
  collection_ids: string[];
  updated_at: string;
  messages: Message[];
}

export interface ChatInput {
  conversation_id?: string | null;
  message: string;
  document_ids: string[];
  collection_ids: string[];
}

export interface ChatMeta {
  conversation_id: string;
  user_message_id: string;
  assistant_message_id: string;
}

export interface ChatDone {
  message_id: string;
  usage: { prompt_tokens: number; completion_tokens: number } | null;
}

export class ApiError extends Error {
  constructor(public status: number, public code: string, message: string) {
    super(message);
    this.name = 'ApiError';
  }
}

export async function request<T>(path: string, options: RequestInit = {}): Promise<T> {
  let response: Response;
  try {
    const headers = new Headers(options.headers);
    if (!headers.has('Accept')) headers.set('Accept', 'application/json');
    response = await fetch(`/api${path}`, {
      ...options,
      headers,
      signal: options.signal ?? AbortSignal.timeout(8000),
    });
  } catch {
    throw new ApiError(0, 'SERVER_UNAVAILABLE', 'No se pudo contactar al servidor Rust. Comprueba que esté en ejecución.');
  }
  if (response.status === 204) return undefined as T;
  let text: string;
  try { text = await response.text(); } catch {
    throw new ApiError(response.status, 'INVALID_RESPONSE', 'El servidor no devolvió una respuesta válida.');
  }
  let data: unknown = undefined;
  if (text.trim()) {
    try { data = JSON.parse(text); } catch {
      throw new ApiError(response.status, 'INVALID_RESPONSE', 'El servidor no devolvió una respuesta JSON válida.');
    }
  }
  if (!response.ok) {
    const error = (data as { error?: { code?: string; message?: string } })?.error;
    throw new ApiError(response.status, error?.code ?? 'REQUEST_FAILED', error?.message ?? 'No se pudo completar la operación.');
  }
  return data as T;
}

export const listDocuments = () => request<Document[]>('/documents');

export async function uploadDocument(file: File, title?: string): Promise<Document> {
  const body = new FormData();
  body.append('file', file);
  if (title?.trim()) body.append('title', title.trim());
  return request<Document>('/documents', {
    method: 'POST',
    body,
    signal: AbortSignal.timeout(60000),
  });
}

export const deleteDocument = (id: string) =>
  request<void>(`/documents/${encodeURIComponent(id)}`, { method: 'DELETE' });

export const documentContent = (id: string, page = 1) =>
  request<{ id: string; page: number; pages: number; chunks: DocumentChunk[] }>(
    `/documents/${encodeURIComponent(id)}/content?page=${page}`,
  );

export const documentFileUrl = (id: string) =>
  `/api/documents/${encodeURIComponent(id)}/file`;

export const documentFilePageUrl = (id: string, page = 1) => `${documentFileUrl(id)}#page=${page}`;

export const listModels = () => request<ModelInfo[]>('/models');

export const saveModel = (model: string) =>
  request<ModelInfo>('/settings/model', {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ model }),
  });

export const listCollections = () => request<Collection[]>('/collections');

export const createCollection = (name: string, description: string) =>
  request<Collection>('/collections', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ name, description }),
  });

export const deleteCollection = (id: string) =>
  request<void>(`/collections/${encodeURIComponent(id)}`, { method: 'DELETE' });

export const addDocumentsToCollection = (collectionId: string, documentIds: string[]) =>
  request<void>(`/collections/${encodeURIComponent(collectionId)}/documents`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ document_ids: documentIds }),
  });

export const listConversations = (offset = 0, limit = 20) =>
  request<Conversation[]>(`/conversations?offset=${offset}&limit=${limit}`);

export const getConversation = (id: string) =>
  request<Conversation>(`/conversations/${encodeURIComponent(id)}`);

export const renameConversation = (id: string, title: string) =>
  request<Conversation>(`/conversations/${encodeURIComponent(id)}`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ title }),
  });

export const deleteConversation = (id: string) =>
  request<void>(`/conversations/${encodeURIComponent(id)}`, { method: 'DELETE' });

export interface ChatStreamHandlers {
  onMeta?: (meta: ChatMeta) => void;
  onDelta?: (content: string) => void;
  onCitations?: (citations: Citation[]) => void;
  onDone?: (done: ChatDone) => void;
  onError?: (error: { code: string; message: string }) => void;
}

/**
 * Consume el SSE de `POST /api/chat` con `fetch` + `ReadableStream`
 * (`EventSource` no admite POST).
 */
export async function streamChat(
  input: ChatInput,
  handlers: ChatStreamHandlers,
  signal?: AbortSignal,
): Promise<void> {
  let response: Response;
  try {
    response = await fetch('/api/chat', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', Accept: 'text/event-stream' },
      body: JSON.stringify(input),
      signal,
    });
  } catch (error) {
    if (error instanceof DOMException && error.name === 'AbortError') throw error;
    throw new ApiError(0, 'SERVER_UNAVAILABLE', 'No se pudo contactar al servidor Rust. Comprueba que esté en ejecución.');
  }
  if (!response.ok) {
    let data: unknown = undefined;
    try { data = await response.json(); } catch { /* sin cuerpo JSON */ }
    const error = (data as { error?: { code?: string; message?: string } })?.error;
    throw new ApiError(response.status, error?.code ?? 'REQUEST_FAILED', error?.message ?? 'No se pudo iniciar la consulta.');
  }
  const reader = response.body?.getReader();
  if (!reader) throw new ApiError(0, 'INVALID_RESPONSE', 'El servidor no devolvió un stream SSE.');
  const decoder = new TextDecoder();
  let buffer = '';
  const flush = (block: string) => {
    const event = dispatchSseBlock(block, handlers);
    return event;
  };
  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    buffer += decoder.decode(value, { stream: true }).replace(/\r\n/g, '\n');
    let index = buffer.indexOf('\n\n');
    while (index >= 0) {
      flush(buffer.slice(0, index));
      buffer = buffer.slice(index + 2);
      index = buffer.indexOf('\n\n');
    }
  }
  buffer += decoder.decode().replace(/\r\n/g, '\n');
  if (buffer.trim()) flush(buffer);
}

export function dispatchSseBlock(block: string, handlers: ChatStreamHandlers): string | null {
  let event = 'message';
  const data: string[] = [];
  for (const line of block.split('\n')) {
    if (!line || line.startsWith(':')) continue;
    if (line.startsWith('event:')) event = line.slice(6).trim();
    else if (line.startsWith('data:')) data.push(line.slice(5).replace(/^ /, ''));
  }
  if (!data.length) return null;
  const raw = data.join('\n');
  let payload: unknown;
  try { payload = JSON.parse(raw); } catch { payload = { content: raw }; }
  switch (event) {
    case 'meta': handlers.onMeta?.(payload as ChatMeta); break;
    case 'delta': handlers.onDelta?.((payload as { content?: string }).content ?? ''); break;
    case 'citations': handlers.onCitations?.((payload as { citations?: Citation[] }).citations ?? []); break;
    case 'done': handlers.onDone?.(payload as ChatDone); break;
    case 'error': handlers.onError?.(payload as { code: string; message: string }); break;
  }
  return event;
}

export const unavailable = 'La conexión documental está pendiente: todavía no se ha definido el esquema ni la ingestión en SoupDB.';

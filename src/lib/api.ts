export type View = 'chat' | 'library' | 'documents' | 'collections' | 'history';
export const views: View[] = ['chat', 'library', 'documents', 'collections', 'history'];
export function viewFromHash(hash: string): View {
  const value = hash.replace(/^#\/?/, '');
  return views.includes(value as View) ? value as View : 'chat';
}

export interface Capabilities {
  driver: string;
  schema_configured: boolean;
  ingestion_available: boolean;
  groq_configured: boolean;
  groq_model: string | null;
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
  let data: unknown;
  try { data = await response.json(); } catch {
    throw new ApiError(response.status, 'INVALID_RESPONSE', 'El servidor no devolvió una respuesta JSON válida.');
  }
  if (!response.ok) {
    const error = (data as { error?: { code?: string; message?: string } })?.error;
    throw new ApiError(response.status, error?.code ?? 'REQUEST_FAILED', error?.message ?? 'No se pudo completar la operación.');
  }
  return data as T;
}

export const unavailable = 'La conexión documental está pendiente: todavía no se ha definido el esquema ni la ingestión en SoupDB.';

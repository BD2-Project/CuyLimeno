import { afterEach, describe, expect, it, vi } from 'vitest';
import { ApiError, dispatchSseBlock, request, streamChat, viewFromHash } from './api';

afterEach(() => vi.unstubAllGlobals());

describe('navegación', () => {
  it('acepta las dos pantallas y recupera rutas antiguas', () => {
    expect(viewFromHash('#/chat')).toBe('chat');
    expect(viewFromHash('#/documents')).toBe('documents');
    expect(viewFromHash('#/settings')).toBe('settings');
    expect(viewFromHash('#/library')).toBe('documents');
    expect(viewFromHash('#/history')).toBe('chat');
    expect(viewFromHash('#/desconocido')).toBe('chat');
  });
});

describe('frontera HTTP', () => {
  it('preserva el error real de esquema sin convertirlo en datos vacíos', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(JSON.stringify({ error: { code: 'DOCUMENT_SCHEMA_NOT_CONFIGURED', message: 'Esquema pendiente' } }), { status: 501 })));
    await expect(request('/documents')).rejects.toMatchObject({ status: 501, code: 'DOCUMENT_SCHEMA_NOT_CONFIGURED', message: 'Esquema pendiente' });
  });
  it('informa cuando el puente no está disponible', async () => {
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new TypeError('fetch failed')));
    await expect(request('/health')).rejects.toBeInstanceOf(ApiError);
    await expect(request('/health')).rejects.toMatchObject({ code: 'SERVER_UNAVAILABLE' });
  });
  it('rechaza HTML accidental del fallback de desarrollo', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response('<html></html>')));
    await expect(request('/capabilities')).rejects.toMatchObject({ code: 'INVALID_RESPONSE' });
  });
  it('entrega datos reales en respuestas exitosas', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(JSON.stringify({ database: 'connected' }))));
    expect(await request('/health')).toEqual({ database: 'connected' });
  });
  it('acepta 204 sin cuerpo', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(null, { status: 204 })));
    await expect(request('/documents/1', { method: 'DELETE' })).resolves.toBeUndefined();
  });
});

describe('SSE del chat', () => {
  it('despacha meta, delta, citations, done y error', () => {
    const seen: string[] = [];
    const handlers = {
      onMeta: (meta: { conversation_id: string }) => seen.push(`meta:${meta.conversation_id}`),
      onDelta: (content: string) => seen.push(`delta:${content}`),
      onCitations: (citations: unknown[]) => seen.push(`citations:${citations.length}`),
      onDone: (done: { message_id: string }) => seen.push(`done:${done.message_id}`),
      onError: (error: { code: string }) => seen.push(`error:${error.code}`),
    };

    expect(dispatchSseBlock('event: meta\ndata: {"conversation_id":"c1","user_message_id":"u1","assistant_message_id":"a1"}', handlers)).toBe('meta');
    expect(dispatchSseBlock('event: delta\ndata: {"content":"Hola "}', handlers)).toBe('delta');
    expect(dispatchSseBlock('event: delta\ndata: {"content":"mundo"}', handlers)).toBe('delta');
    expect(dispatchSseBlock('event: citations\ndata: {"citations":[{"document_id":"d1"}]}', handlers)).toBe('citations');
    expect(dispatchSseBlock('event: done\ndata: {"message_id":"a1","usage":null}', handlers)).toBe('done');
    expect(dispatchSseBlock('event: error\ndata: {"code":"GROQ_ERROR","message":"falló"}', handlers)).toBe('error');
    expect(dispatchSseBlock(': comentario\n\n', handlers)).toBeNull();

    expect(seen).toEqual(['meta:c1', 'delta:Hola ', 'delta:mundo', 'citations:1', 'done:a1', 'error:GROQ_ERROR']);
  });

  it('streamChat consume el cuerpo SSE y acumula deltas', async () => {
    const encoder = new TextEncoder();
    const body = new ReadableStream({
      start(controller) {
        controller.enqueue(encoder.encode('event: meta\ndata: {"conversation_id":"c1","user_message_id":"u1","assistant_message_id":"a1"}\n\n'));
        controller.enqueue(encoder.encode('event: delta\ndata: {"content":"Hola"}\n\nevent: delta\ndata: {"content":" mundo"}\n\n'));
        controller.enqueue(encoder.encode('event: done\ndata: {"message_id":"a1","usage":{"prompt_tokens":3,"completion_tokens":2}}\n\n'));
        controller.close();
      },
    });
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(body, { status: 200, headers: { 'Content-Type': 'text/event-stream' } })));

    const deltas: string[] = [];
    let meta = '';
    let done = false;
    await streamChat(
      { message: 'pregunta', document_ids: [], collection_ids: [] },
      {
        onMeta: (value) => { meta = value.conversation_id; },
        onDelta: (value) => deltas.push(value),
        onDone: () => { done = true; },
      },
    );
    expect(meta).toBe('c1');
    expect(deltas.join('')).toBe('Hola mundo');
    expect(done).toBe(true);
  });

  it('propaga el sobre de error JSON antes del stream', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(JSON.stringify({ error: { code: 'GENERATION_UNAVAILABLE', message: 'Sin Groq' } }), { status: 503 })));
    await expect(streamChat({ message: 'x', document_ids: [], collection_ids: [] }, {})).rejects.toMatchObject({ code: 'GENERATION_UNAVAILABLE', status: 503 });
  });
});

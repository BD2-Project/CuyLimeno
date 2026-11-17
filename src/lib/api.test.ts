import { afterEach, describe, expect, it, vi } from 'vitest';
import { ApiError, request, viewFromHash } from './api';

afterEach(() => vi.unstubAllGlobals());

describe('navegación', () => {
  it('acepta las cinco pantallas y recupera rutas desconocidas', () => {
    for (const view of ['chat', 'library', 'documents', 'collections', 'history']) {
      expect(viewFromHash(`#/${view}`)).toBe(view);
    }
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
});

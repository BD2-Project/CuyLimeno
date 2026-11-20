import { test, expect } from '@playwright/test';
import { readFileSync } from 'node:fs';

const samplePdf = readFileSync(new URL('../fixtures/sample.pdf', import.meta.url));

test.beforeEach(async ({ page }) => {
  await page.route('**/api/**', (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path === '/api/health') return route.fulfill({ json: { status: 'ok', database: 'connected', driver: 'rsoup' } });
    if (path === '/api/capabilities') return route.fulfill({ json: { driver: 'rsoup', schema_configured: false, ingestion_available: false, retrieval_available: false, generation_available: false, groq_configured: false, groq_model: null } });
    return route.fulfill({ status: 501, json: { error: { code: 'DOCUMENT_SCHEMA_NOT_CONFIGURED', message: 'El esquema documental está pendiente.' } } });
  });
});

const readyCapabilities = {
  driver: 'rsoup',
  health_check: 'rsoup_ping',
  schema_configured: true,
  ingestion_available: true,
  retrieval_available: true,
  generation_available: true,
  authentication_available: false,
  groq_configured: true,
  groq_model: 'test-model',
};

function sampleDocument(overrides: Record<string, unknown> = {}) {
  return {
    id: 'doc-1',
    title: 'Mi paper',
    authors: ['Ada Lovelace'],
    year: 2026,
    filename: 'paper.pdf',
    page_count: 12,
    collection_ids: [],
    created_at: '2026-11-20T00:00:00Z',
    status: 'ready',
    ...overrides,
  };
}

function sampleConversation(overrides: Record<string, unknown> = {}) {
  return {
    id: 'conv-1',
    title: 'Conversación de prueba',
    document_ids: [],
    collection_ids: [],
    updated_at: '2026-11-20T00:00:00Z',
    messages: [],
    ...overrides,
  };
}

async function mockWorkspace(
  page: import('@playwright/test').Page,
  initialDocuments: unknown[],
  options: { conversations?: unknown[]; onUpload?: () => unknown } = {},
) {
  const stored = [...initialDocuments];
  await page.route('**/api/capabilities', (route) => route.fulfill({ json: readyCapabilities }));
  await page.route('**/api/health', (route) => route.fulfill({ json: { status: 'ok', database: 'connected', driver: 'rsoup' } }));
  await page.route('**/api/documents', async (route) => {
    if (route.request().method() === 'POST') {
      const created = options.onUpload?.() ?? sampleDocument({ status: 'processing', page_count: null });
      stored.unshift(created);
      return route.fulfill({ status: 201, json: created });
    }
    return route.fulfill({ json: stored });
  });
  await page.route('**/api/conversations**', (route) => route.fulfill({ json: options.conversations ?? [] }));
}

/** Respuesta SSE del chat con una única respuesta citada. */
function chatSse(options: { citations?: unknown[]; content?: string; error?: boolean } = {}) {
  const events = [
    'event: meta',
    'data: {"conversation_id":"c1","user_message_id":"u1","assistant_message_id":"a1"}',
    '',
  ];
  if (options.error) {
    events.push('event: error', 'data: {"code":"GROQ_ERROR","message":"Groq no disponible"}', '');
  } else {
    events.push('event: delta', `data: ${JSON.stringify({ content: options.content ?? 'Según la fuente [1], la conclusión es X.' })}`, '');
    events.push('event: citations', `data: ${JSON.stringify({ citations: options.citations ?? [] })}`, '');
    events.push('event: done', 'data: {"message_id":"a1","usage":null}', '');
  }
  events.push('');
  return events.join('\n');
}

test('las tres rutas respetan el alcance sin datos simulados', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  for (const [route, title] of Object.entries({ chat: 'Tu próxima idea empieza', documents: 'Mis documentos', settings: 'Mi espacio' })) {
    await page.goto(`/#/${route}`);
    await expect(page.getByRole('heading', { level: 1 })).toContainText(title);
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
  }
  expect(errors).toEqual([]);
});

test('el borrador no envía consultas y el panel de fuentes es inline', async ({ page }) => {
  await page.goto('/#/chat');
  await page.getByRole('button', { name: 'Resume los aportes principales de mis lecturas' }).click();
  await expect(page.getByRole('textbox', { name: 'Tu pregunta académica' })).toHaveValue('Resume los aportes principales de mis lecturas');
  await expect(page.getByRole('button', { name: 'Consultar', exact: true })).toBeDisabled();
  await page.getByRole('tab', { name: 'Fuentes' }).click();
  await expect(page.getByRole('tab', { name: 'Fuentes' })).toHaveAttribute('aria-selected', 'true');
  await expect(page.getByText('No hay documentos')).toBeVisible();
});

test('saltar al contenido conserva la ruta de documentos', async ({ page }) => {
  await page.goto('/#/documents');
  const skip = page.getByRole('link', { name: 'Saltar al contenido' });
  await skip.focus();
  await skip.press('Enter');
  await expect(page).toHaveURL(/#\/documents$/);
  await expect(page.locator('main')).toBeFocused();
});

test('la carga explica su dependencia y no selecciona archivos', async ({ page }) => {
  await page.goto('/#/documents');
  await page.getByRole('button', { name: 'Añadir documentos' }).click();
  await expect(page.getByRole('heading', { name: 'Ingestión de PDF pendiente' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Seleccionar PDF' })).toBeDisabled();
  await expect(page.locator('input[type=file]')).toHaveCount(0);
});

test('la caída del puente se muestra en configuración sin aparentar conexión', async ({ page }) => {
  await page.route('**/api/health', (route) => route.abort());
  await page.goto('/#/settings');
  await expect(page.getByRole('heading', { name: 'Mi espacio' })).toBeVisible();
  await expect(page.getByText('No se pudo contactar al servidor Rust')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Comprobar conexión' })).toBeEnabled();
});

test('el menú móvil navega a documentos y enfoca el contenido', async ({ page }, testInfo) => {
  test.skip(testInfo.project.name !== 'mobile', 'Solo aplica al drawer móvil');
  await page.goto('/#/chat');
  await page.getByRole('button', { name: 'Abrir navegación' }).click();
  await expect(page.locator('.brand')).toBeFocused();
  await page.getByRole('button', { name: 'Mis documentos', exact: true }).click();
  await expect(page).toHaveURL(/#\/documents$/);
  await expect(page.locator('main')).toBeFocused();
});

test('las sugerencias piden confirmación y permiten conservar el borrador', async ({ page }) => {
  await page.goto('/#/chat');
  const question = page.getByRole('textbox', { name: 'Tu pregunta académica' });
  await question.fill('Mi pregunta original');
  const suggestion = page.getByRole('button', { name: 'Compara las metodologías de los documentos' });
  await suggestion.click();
  await expect(page.getByRole('dialog')).toContainText('¿Reemplazar tu borrador?');
  await expect(question).toHaveValue('Mi pregunta original');
  await page.getByRole('button', { name: 'Conservar borrador' }).click();
  await expect(question).toHaveValue('Mi pregunta original');
  await expect(suggestion).toBeFocused();
  await suggestion.click();
  await page.keyboard.press('Escape');
  await expect(question).toHaveValue('Mi pregunta original');
  await suggestion.click();
  await page.getByRole('button', { name: 'Reemplazar borrador', exact: true }).click();
  await expect(question).toHaveValue('Compara las metodologías de los documentos');
});

test('Nueva conversación conserva el borrador', async ({ page }, testInfo) => {
  await page.goto('/#/chat');
  const question = page.getByRole('textbox', { name: 'Tu pregunta académica' });
  await question.fill('Texto que quiero conservar');
  if (testInfo.project.name === 'mobile') await page.getByRole('button', { name: 'Abrir navegación' }).click();
  await page.getByRole('button', { name: 'Nueva conversación', exact: true }).click();
  await expect(question).toHaveValue('Texto que quiero conservar');
});

test('el esquema pendiente no se presenta como fallo recuperable', async ({ page }) => {
  await page.goto('/#/documents');
  await expect(page.getByRole('status').filter({ hasText: 'Integración documental pendiente' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Reintentar', exact: true })).toHaveCount(0);
});

test('la configuración refleja la comprobación de conexión', async ({ page }) => {
  let finish: () => void = () => {};
  const pending = new Promise<void>((resolve) => { finish = resolve; });
  await page.route('**/api/health', async (route) => { await pending; await route.fulfill({ json: { database: 'connected' } }); });
  await page.goto('/#/settings');
  const database = page.locator('.connection-details > div').filter({ hasText: 'SoupDB / rsoup' });
  await expect(database).toContainText('Comprobando…');
  finish();
  await expect(database).toContainText('Conectado');
});

test('reflujo a 320 px y menú desplazable en poca altura', async ({ page }) => {
  await page.setViewportSize({ width: 320, height: 320 });
  for (const route of ['chat', 'documents', 'settings']) {
    await page.goto(`/#/${route}`);
    await expect(page.getByRole('heading', { level: 1 })).toBeVisible();
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
  }
  await page.getByRole('button', { name: 'Abrir navegación' }).click();
  const documents = page.getByRole('button', { name: 'Mis documentos', exact: true });
  await documents.scrollIntoViewIfNeeded();
  await expect(documents).toBeInViewport();
  await documents.click();
  await expect(page.locator('main')).toBeFocused();
});

test('sube un PDF real y lo refleja en el catálogo', async ({ page }) => {
  await mockWorkspace(page, [], { onUpload: () => sampleDocument({ id: 'doc-up', title: 'Paper subido', status: 'processing', page_count: null }) });
  await page.goto('/#/documents');
  await page.getByRole('button', { name: 'Añadir documentos' }).first().click();
  const input = page.locator('input[type=file]');
  await expect(input).toHaveCount(1);
  await input.setInputFiles({ name: 'paper.pdf', mimeType: 'application/pdf', buffer: Buffer.from('%PDF-1.4\n') });
  await expect(page.getByRole('status').filter({ hasText: 'Documento en cola de procesamiento' })).toBeVisible();
  await expect(page.getByText('Paper subido')).toBeVisible();
});

test('el catálogo muestra los documentos y su estado reales', async ({ page }) => {
  await mockWorkspace(page, [sampleDocument()]);
  await page.goto('/#/documents');
  await expect(page.getByText('Mi paper')).toBeVisible();
  await expect(page.getByText('Ada Lovelace', { exact: false })).toBeVisible();
  await expect(page.getByText('Listo', { exact: true })).toBeVisible();
  await expect(page.getByText('No hay datos de demostración')).toHaveCount(0);
});

test('el chat transmite la respuesta y publica las citas en el inspector', async ({ page }) => {
  await mockWorkspace(page, [sampleDocument()]);
  await page.route('**/api/chat', (route) => route.fulfill({
    status: 200,
    headers: { 'Content-Type': 'text/event-stream' },
    body: chatSse({ content: 'Según la fuente [1], **la conclusión es X**:\n\n- Primer punto\n- Segundo punto', citations: [{ document_id: 'doc-1', document_title: 'Mi paper', page: 3, excerpt: 'fragmento verificable' }] }),
  }));
  await page.goto('/#/chat');
  await page.getByRole('textbox', { name: 'Tu pregunta académica' }).fill('¿Qué es SoupDB?');
  await page.getByRole('button', { name: 'Consultar', exact: true }).click();
  await expect(page.locator('.message-content.markdown strong')).toHaveText('la conclusión es X');
  await expect(page.locator('.message-content.markdown ul li')).toHaveCount(2);
  await expect(page.locator('.message-content.markdown ul li').first()).toHaveCSS('list-style-type', 'disc');
  await expect(page.locator('.evidence-item').filter({ hasText: 'Mi paper' })).toBeVisible();
  await expect(page.getByText('fragmento verificable')).toBeVisible();
  await expect(page.getByText('Espacio personal · Sin cuentas compartidas')).toHaveCount(0);
});

test('el chat responde sin citas y lo marca como respuesta general', async ({ page }) => {
  await mockWorkspace(page, [sampleDocument()]);
  await page.route('**/api/chat', (route) => route.fulfill({
    status: 200,
    headers: { 'Content-Type': 'text/event-stream' },
    body: chatSse({ content: 'Puedo darte una respuesta general: revisa tus fuentes para más detalle.' }),
  }));
  await page.goto('/#/chat');
  await page.getByRole('textbox', { name: 'Tu pregunta académica' }).fill('¿De qué tratan mis documentos?');
  await page.getByRole('button', { name: 'Consultar', exact: true }).click();
  await expect(page.getByText('Puedo darte una respuesta general: revisa tus fuentes para más detalle.')).toBeVisible();
  await expect(page.locator('.badge.general')).toHaveText('Respuesta general');
});

test('muestra el error del stream con opción de reintentar', async ({ page }) => {
  await mockWorkspace(page, [sampleDocument()]);
  await page.route('**/api/chat', (route) => route.fulfill({
    status: 200,
    headers: { 'Content-Type': 'text/event-stream' },
    body: chatSse({ error: true }),
  }));
  await page.goto('/#/chat');
  await page.getByRole('textbox', { name: 'Tu pregunta académica' }).fill('Pregunta');
  await page.getByRole('button', { name: 'Consultar', exact: true }).click();
  await expect(page.getByRole('alert')).toContainText('Groq no disponible');
  await expect(page.getByRole('button', { name: 'Reintentar' })).toBeVisible();
});

test('abre el PDF real, navega páginas y lo usa como fuente', async ({ page }) => {
  await mockWorkspace(page, [sampleDocument()]);
  await page.route('**/api/documents/doc-1/file', (route) => route.fulfill({ body: samplePdf, contentType: 'application/pdf' }));
  await page.goto('/#/documents');
  await page.getByRole('button', { name: 'Ver Mi paper' }).click();
  const dialog = page.getByRole('dialog');
  await expect(dialog).toContainText('Página 1 de 3');
  await dialog.getByRole('button', { name: 'Siguiente ›' }).click();
  await expect(dialog).toContainText('Página 2 de 3');
  await page.getByRole('button', { name: 'Usar en el chat' }).click();
  await expect(page).toHaveURL(/#\/chat$/);
  await expect(page.getByText('1 seleccionada', { exact: true })).toBeVisible();
});

test('al pulsar una cita abre el PDF en la página y resalta el fragmento', async ({ page }) => {
  await mockWorkspace(page, [sampleDocument()]);
  await page.route('**/api/documents/doc-1/file', (route) => route.fulfill({ body: samplePdf, contentType: 'application/pdf' }));
  await page.route('**/api/chat', (route) => route.fulfill({
    status: 200,
    headers: { 'Content-Type': 'text/event-stream' },
    body: chatSse({
      citations: [{
        document_id: 'doc-1',
        document_title: 'Mi paper',
        page: 3,
        excerpt: 'Fragmento citado verificable sobre el motor',
      }],
    }),
  }));
  await page.goto('/#/chat');
  await page.getByRole('textbox', { name: 'Tu pregunta académica' }).fill('¿Cuál es la conclusión?');
  await page.getByRole('button', { name: 'Consultar', exact: true }).click();
  await page.locator('.evidence-item').filter({ hasText: 'Mi paper' }).click();
  const dialog = page.getByRole('dialog');
  await expect(dialog).toContainText('Página 3 de 3');
  await expect(dialog.locator('.pdf-highlight').first()).toBeVisible();
  await expect(dialog).toContainText('Fragmento citado verificable sobre el motor');
});

test('el panel de fuentes selecciona y deselecciona documentos al instante', async ({ page }) => {
  await mockWorkspace(page, [sampleDocument()]);
  await page.goto('/#/chat');
  await page.getByRole('tab', { name: 'Fuentes' }).click();
  const checkbox = page.locator('.inspector-sources input[type=checkbox]').first();
  await checkbox.check();
  await expect(page.getByText('1 seleccionada', { exact: true })).toBeVisible();
  await checkbox.uncheck();
  await expect(page.getByText('Se usa la biblioteca completa', { exact: true })).toBeVisible();
});

test('renombra y elimina conversaciones desde el menú lateral', async ({ page }, testInfo) => {
  await mockWorkspace(page, [sampleDocument()], { conversations: [sampleConversation()] });
  await page.route('**/api/conversations/conv-1', async (route) => {
    if (route.request().method() === 'PATCH') {
      return route.fulfill({ json: sampleConversation({ title: 'Nuevo nombre' }) });
    }
    if (route.request().method() === 'DELETE') return route.fulfill({ status: 204 });
    return route.fulfill({ json: sampleConversation() });
  });
  await page.goto('/#/chat');
  if (testInfo.project.name === 'mobile') await page.getByRole('button', { name: 'Abrir navegación' }).click();
  await page.getByRole('button', { name: 'Opciones de Conversación de prueba' }).click();
  await page.getByRole('button', { name: 'Renombrar' }).click();
  await page.getByLabel('Título de la conversación').fill('Nuevo nombre');
  await page.getByRole('button', { name: 'Guardar' }).click();
  await expect(page.getByText('Nuevo nombre')).toBeVisible();
  page.on('dialog', (dialog) => dialog.accept());
  await page.getByRole('button', { name: 'Opciones de Nuevo nombre' }).click();
  await page.getByRole('button', { name: 'Eliminar' }).click();
  await expect(page.getByText('Nuevo nombre')).toHaveCount(0);
});

test('carga más conversaciones de forma paginada', async ({ page }, testInfo) => {
  const firstPage = Array.from({ length: 20 }, (_, index) => sampleConversation({ id: `conv-${index}`, title: `Chat ${index}` }));
  await mockWorkspace(page, [], { conversations: firstPage });
  await page.route('**/api/conversations?offset=20&limit=20', (route) => route.fulfill({
    json: [sampleConversation({ id: 'conv-20', title: 'Chat 20' })],
  }));
  await page.goto('/#/chat');
  if (testInfo.project.name === 'mobile') await page.getByRole('button', { name: 'Abrir navegación' }).click();
  await expect(page.getByText('Chat 19')).toBeVisible();
  await page.getByRole('button', { name: 'Cargar más' }).click();
  await expect(page.getByText('Chat 20')).toBeVisible();
});

test('cambia el modelo de generación desde configuración', async ({ page }) => {
  await mockWorkspace(page, [sampleDocument()]);
  await page.route('**/api/models', (route) => route.fulfill({
    json: [
      { id: 'openai/gpt-oss-20b', context_window: 128000, owned_by: 'OpenAI' },
      { id: 'openai/gpt-oss-120b', context_window: 128000, owned_by: 'OpenAI' },
    ],
  }));
  await page.route('**/api/settings/model', async (route) => {
    const body = route.request().postDataJSON() as { model: string };
    return route.fulfill({ json: { id: body.model, context_window: 128000 } });
  });
  await page.goto('/#/settings');
  await expect(page.getByRole('heading', { name: 'Mi espacio' })).toBeVisible();
  await page.getByLabel('Modelo activo').selectOption('openai/gpt-oss-120b');
  await page.getByRole('button', { name: 'Guardar modelo' }).click();
  await expect(page.getByRole('status').filter({ hasText: 'Modelo de generación actualizado' })).toBeVisible();
});

test('Enter envía el mensaje y Ctrl+Enter inserta un salto de línea', async ({ page }) => {
  await mockWorkspace(page, [sampleDocument()]);
  await page.route('**/api/chat', (route) => route.fulfill({
    status: 200,
    headers: { 'Content-Type': 'text/event-stream' },
    body: chatSse({ content: 'Respuesta de prueba.' }),
  }));
  await page.goto('/#/chat');
  const box = page.getByRole('textbox', { name: 'Tu pregunta académica' });
  await box.fill('Primera línea');
  await box.press('Control+Enter');
  await expect(box).toHaveValue('Primera línea\n');
  await box.press('Enter');
  await expect(box).toHaveValue('');
  await expect(page.locator('.message-content.markdown').filter({ hasText: 'Primera línea' })).toBeVisible();
});

test('el menú de una conversación al final se muestra por encima y el listado se difumina', async ({ page }, testInfo) => {
  const many = Array.from({ length: 20 }, (_, index) => sampleConversation({ id: `conv-${index}`, title: `Chat ${index}` }));
  await mockWorkspace(page, [], { conversations: many });
  await page.goto('/#/chat');
  if (testInfo.project.name === 'mobile') await page.getByRole('button', { name: 'Abrir navegación' }).click();
  await expect(page.locator('.conversation-list')).toHaveClass(/fade-bottom/);
  const menu = page.getByRole('button', { name: 'Opciones de Chat 19' });
  await menu.scrollIntoViewIfNeeded();
  await menu.click();
  const popover = page.locator('.floating-popover');
  await expect(popover).toBeVisible();
  await expect(popover).toHaveCSS('position', 'fixed');
  await expect(popover).toBeInViewport();
});

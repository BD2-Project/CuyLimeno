import { test, expect } from '@playwright/test';

test.beforeEach(async ({ page }) => {
  await page.route('**/api/**', (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path === '/api/health') return route.fulfill({ json: { status: 'ok', database: 'connected', driver: 'rsoup' } });
    if (path === '/api/capabilities') return route.fulfill({ json: { driver: 'rsoup', schema_configured: false, ingestion_available: false, groq_configured: false, groq_model: null } });
    return route.fulfill({ status: 501, json: { error: { code: 'DOCUMENT_SCHEMA_NOT_CONFIGURED', message: 'El esquema documental está pendiente.' } } });
  });
});

test('las cinco rutas respetan el alcance sin datos simulados', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  for (const [route, title] of Object.entries({ chat: 'Tu próxima idea empieza', library: 'Biblioteca personal', documents: 'Mis documentos', collections: 'Colecciones', history: 'Historial' })) {
    await page.goto(`/#/${route}`);
    await expect(page.getByRole('heading', { level: 1 })).toContainText(title);
    await expect(page.getByRole('button', { name: 'SoupDB conectado' })).toBeVisible();
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
  }
  expect(errors).toEqual([]);
});

test('el borrador no envía consultas y el diálogo conserva el foco', async ({ page }) => {
  await page.goto('/#/chat');
  await page.getByRole('button', { name: 'Resume los aportes principales de mis lecturas' }).click();
  await expect(page.getByRole('textbox', { name: 'Tu pregunta académica' })).toHaveValue('Resume los aportes principales de mis lecturas');
  await expect(page.getByRole('button', { name: 'Consultar', exact: true })).toBeDisabled();
  const sourceButton = page.getByRole('button', { name: 'Seleccionar fuentes' });
  await sourceButton.click();
  await expect(page.getByRole('dialog')).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.getByRole('dialog')).not.toBeVisible();
  await expect(sourceButton).toBeFocused();
});

test('saltar al contenido conserva la ruta de historial', async ({ page }) => {
  await page.goto('/#/history');
  const skip = page.getByRole('link', { name: 'Saltar al contenido' });
  await skip.focus();
  await skip.press('Enter');
  await expect(page).toHaveURL(/#\/history$/);
  await expect(page.locator('main')).toBeFocused();
});

test('la carga explica su dependencia y no selecciona archivos', async ({ page }) => {
  await page.goto('/#/documents');
  await page.getByRole('button', { name: 'Añadir documentos' }).click();
  await expect(page.getByRole('heading', { name: 'Ingestión de PDF pendiente' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Seleccionar PDF' })).toBeDisabled();
  await expect(page.locator('input[type=file]')).toHaveCount(0);
});

test('la caída del puente ofrece un reintento sin aparentar conexión', async ({ page }) => {
  await page.route('**/api/health', (route) => route.abort());
  await page.goto('/#/chat');
  await page.getByRole('button', { name: 'Sin conexión a SoupDB' }).click();
  await expect(page.getByRole('dialog')).toContainText('No se pudo contactar al servidor Rust');
  await expect(page.getByRole('button', { name: 'Comprobar conexión', exact: true })).toBeEnabled();
});

test('el menú móvil encierra el foco y navega a un elemento visible', async ({ page }, testInfo) => {
  test.skip(testInfo.project.name !== 'mobile', 'Solo aplica al drawer móvil');
  await page.goto('/#/chat');
  await page.getByRole('button', { name: 'Abrir navegación' }).click();
  await expect(page.locator('.brand')).toBeFocused();
  await page.keyboard.press('Shift+Tab');
  await expect(page.locator('.main-nav button').last()).toBeFocused();
  await page.keyboard.press('Tab');
  await expect(page.locator('.brand')).toBeFocused();
  await page.getByRole('button', { name: 'Historial de consultas', exact: true }).click();
  await expect(page).toHaveURL(/#\/history$/);
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

test('ir a documentos desde fuentes enfoca la nueva pantalla', async ({ page }) => {
  await page.goto('/#/chat');
  await page.getByRole('button', { name: 'Seleccionar fuentes' }).click();
  await page.getByRole('button', { name: 'Ir a mis documentos' }).click();
  await expect(page.getByRole('dialog')).not.toBeVisible();
  await expect(page).toHaveURL(/#\/documents$/);
  await expect(page.locator('main')).toBeFocused();
});

test('el esquema pendiente no se presenta como fallo recuperable', async ({ page }) => {
  await page.goto('/#/documents');
  await expect(page.getByRole('status').filter({ hasText: 'Integración documental pendiente' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Reintentar', exact: true })).toHaveCount(0);
});

test('ambos indicadores reflejan la comprobación de conexión', async ({ page }) => {
  let finish: () => void = () => {};
  const pending = new Promise<void>((resolve) => { finish = resolve; });
  await page.route('**/api/health', async (route) => { await pending; await route.fulfill({ json: { database: 'connected' } }); });
  await page.goto('/#/chat');
  await expect(page.locator('.connection-status')).toContainText('COMPROBANDO CONEXIÓN');
  await expect(page.getByRole('button', { name: 'Comprobando conexión…' })).toBeVisible();
  finish();
  await expect(page.locator('.connection-status')).toContainText('SERVICIO CONECTADO');
});

test('reflujo a 320 px y menú desplazable en poca altura', async ({ page }) => {
  await page.setViewportSize({ width: 320, height: 320 });
  for (const route of ['chat', 'library', 'documents', 'collections', 'history']) {
    await page.goto(`/#/${route}`);
    await expect(page.getByRole('heading', { level: 1 })).toBeVisible();
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
  }
  await page.getByRole('button', { name: 'Abrir navegación' }).click();
  const history = page.getByRole('button', { name: 'Historial de consultas', exact: true });
  await history.scrollIntoViewIfNeeded();
  await expect(history).toBeInViewport();
  await history.click();
  await expect(page.locator('main')).toBeFocused();
});

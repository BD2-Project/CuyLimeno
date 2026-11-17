<script lang="ts">
  import { onMount } from 'svelte';
  import Shell from './lib/components/Shell.svelte';
  import Icon from './lib/components/Icon.svelte';
  import { request, viewFromHash, unavailable, type View, type Capabilities } from './lib/api';

  let view = $state<View>(viewFromHash(window.location.hash));
  let capabilities = $state<Capabilities | null>(null);
  let connection = $state<'checking' | 'connected' | 'disconnected'>('checking');
  let connectionError = $state('');
  let resourceError = $state('');
  let loading = $state(false);
  let refreshing = $state(false);
  let search = $state('');
  let statusFilter = $state('all');
  let layout = $state<'grid' | 'list'>('list');
  let draft = $state('');
  let modal = $state<'upload' | 'collection' | 'connection' | 'sources' | null>(null);
  let collectionName = $state('');
  let notice = $state('');
  let requestId = 0;
  const titles: Record<View, string> = { chat: 'Chat académico', library: 'Biblioteca personal', documents: 'Mis documentos', collections: 'Colecciones', history: 'Historial' };
  const endpoints: Partial<Record<View, string>> = { library: '/documents', documents: '/documents', collections: '/collections', history: '/conversations' };

  async function refreshConnection() {
    if (refreshing) return;
    refreshing = true;
    connection = 'checking';
    const [caps, health] = await Promise.allSettled([
      request<Capabilities>('/capabilities'), request<{ database: string }>('/health'),
    ]);
    capabilities = caps.status === 'fulfilled' ? caps.value : null;
    connection = health.status === 'fulfilled' && health.value.database === 'connected' ? 'connected' : 'disconnected';
    connectionError = health.status === 'rejected' ? String(health.reason.message) : '';
    refreshing = false;
  }

  async function loadResource(target: View) {
    const id = ++requestId;
    resourceError = '';
    const endpoint = endpoints[target];
    loading = Boolean(endpoint);
    if (!endpoint) return;
    try { await request(endpoint); }
    catch (error) { if (id === requestId) resourceError = error instanceof Error ? error.message : 'No se pudo cargar el recurso.'; }
    finally { if (id === requestId) loading = false; }
  }

  function navigate(target: View) {
    view = target;
    window.location.hash = `/${target}`;
    search = '';
    statusFilter = 'all';
    notice = '';
    void loadResource(target);
  }

  onMount(() => {
    void refreshConnection();
    void loadResource(view);
    const handleHash = () => {
      const next = viewFromHash(window.location.hash);
      if (next !== view) navigate(next);
    };
    window.addEventListener('hashchange', handleHash);
    return () => window.removeEventListener('hashchange', handleHash);
  });

  async function copyDraft() {
    try { await navigator.clipboard.writeText(draft); notice = 'Borrador copiado.'; }
    catch { notice = 'No se pudo acceder al portapapeles. Puedes seleccionar y copiar el texto.'; }
  }
</script>

<svelte:head><title>{titles[view]} · CuyLimeño</title></svelte:head>

<Shell active={view} onNavigate={navigate} connected={connection === 'connected'}>
  <div class="workspace-topline">
    <div class="breadcrumb">Mi espacio <span>/</span> <strong>{titles[view]}</strong></div>
    <button class="connection-indicator" onclick={() => modal = 'connection'}>
      <span class:online={connection === 'connected'} class="status-dot"></span>
      {connection === 'checking' ? 'Comprobando conexión…' : connection === 'connected' ? 'SoupDB conectado' : 'Sin conexión a SoupDB'}
      <Icon name="chevron-down" size={14} />
    </button>
  </div>

  {#if view === 'chat'}
    <div class="scope-ribbon">
      <span class="eyebrow"><Icon name="filter" size={15} /> Alcance activo</span>
      <span class="chip">Biblioteca personal</span>
      <button class="button ghost compact" onclick={() => modal = 'sources'}><Icon name="plus" size={15} /> Seleccionar fuentes</button>
      <span class="scope-caption">Sin documentos vinculados</span>
    </div>
    <div class="research-layout">
      <section class="research-chat" aria-label="Conversación académica">
        <div class="chat-heading"><div><span class="eyebrow">INVESTIGA CON EVIDENCIA</span><h1>Tu próxima idea empieza<br />con una buena pregunta.</h1><p class="muted">Conecta tus lecturas, contrasta ideas y vuelve siempre a la fuente.</p></div><span class="personal-mark"><Icon name="sparkles" size={22} /></span></div>
        <div class="chat-welcome card">
          <div class="welcome-icon"><Icon name="sparkles" size={25} /></div>
          <h2>Un espacio para pensar con tus documentos</h2>
          <p class="muted">Cada respuesta podrá estar acompañada por fragmentos verificables de tus fuentes académicas.</p>
          <div class="suggestions">
            {#each ['Resume los aportes principales de mis lecturas', 'Compara las metodologías de los documentos', '¿Qué limitaciones señalan los autores?'] as suggestion}
              <button onclick={() => draft = suggestion}><Icon name="arrow-up-right" size={17} />{suggestion}</button>
            {/each}
          </div>
          <p class="fine-print">Las sugerencias solo preparan un borrador. No se envía información.</p>
        </div>
        <div class="composer-wrap">
          <div class="info-banner"><Icon name="info" size={18} /><span>Primero conecta tus fuentes. La ingestión y el esquema documental están pendientes.</span></div>
          <div class="question-composer">
            <label class="sr-only" for="question">Tu pregunta académica</label>
            <textarea id="question" bind:value={draft} placeholder="¿Qué quieres explorar en tus documentos?" rows="3" maxlength="12000"></textarea>
            <div class="composer-actions"><span><Icon name="file-text" size={15} /> Sin fuentes disponibles</span><div><button class="button ghost compact" disabled={!draft.trim()} onclick={copyDraft} aria-label="Copiar borrador"><Icon name="copy" size={16} /></button><button class="button primary compact" disabled title={unavailable}><Icon name="arrow-up" size={17} /> Consultar</button></div></div>
          </div>
          <div class="composer-note"><Icon name="shield" size={13} /> Verifica siempre las citas en el documento original. <span>Generación: GroqCloud</span></div>
          {#if notice}<p class="notice" role="status">{notice}</p>{/if}
        </div>
      </section>
      <aside class="evidence-panel" aria-label="Inspector de evidencia">
        <div class="inspector-title"><Icon name="quote" size={20} /><h2>Evidencia y citas</h2><span class="badge personal">Personal</span></div>
        <p class="muted small">El contexto que sostiene tu investigación.</p>
        <div class="evidence-empty"><div class="outlined-icon"><Icon name="file-search" size={30} /></div><h3>Cada afirmación, una fuente</h3><p>Selecciona una cita en la conversación para revisar el fragmento y su procedencia.</p></div>
        <div class="evidence-guide"><span class="eyebrow">ASÍ FUNCIONARÁ</span><ol><li>Incorpora tus documentos académicos.</li><li>Elige las fuentes de tu consulta.</li><li>Explora la respuesta y contrasta sus citas.</li></ol></div>
        <div class="inspector-bottom"><Icon name="lock" size={16} /><span>Espacio personal · Sin cuentas compartidas</span></div>
      </aside>
    </div>
  {:else}
    <section class="page-content">
      <div class="page-heading">
        <div><span class="eyebrow">TU ESPACIO DE INVESTIGACIÓN</span><h1>{titles[view]}</h1><p class="muted">{view === 'documents' ? 'Organiza tus lecturas y prepara las fuentes de tu próxima investigación.' : view === 'library' ? 'Encuentra y consulta todos tus documentos académicos en un solo lugar.' : view === 'collections' ? 'Dale estructura a tus proyectos, una colección a la vez.' : 'Vuelve a tus preguntas y recupera el contexto de cada investigación.'}</p></div>
        {#if view === 'documents' || view === 'library'}<button class="button primary" onclick={() => modal = 'upload'}><Icon name="upload" size={17} /> Añadir documentos</button>
        {:else if view === 'collections'}<button class="button primary" onclick={() => modal = 'collection'}><Icon name="plus" size={17} /> Nueva colección</button>
        {:else}<button class="button primary" onclick={() => navigate('chat')}><Icon name="plus" size={17} /> Nueva consulta</button>{/if}
      </div>

      <div class="overview-grid">
        <div class="overview-card"><span class="overview-icon"><Icon name={view === 'history' ? 'messages' : view === 'collections' ? 'folder' : 'file-text'} size={21} /></span><div><span class="muted small">{view === 'history' ? 'Conversaciones' : view === 'collections' ? 'Colecciones personales' : 'Documentos personales'}</span><strong>—</strong></div><span class="badge personal">Pendiente</span></div>
        <div class="overview-card"><span class="overview-icon"><Icon name="quote" size={21} /></span><div><span class="muted small">{view === 'history' ? 'Fuentes preservadas' : 'Disponibles para consulta'}</span><strong>—</strong></div><span class="small muted">Sin datos conectados</span></div>
        <div class="overview-card"><span class="overview-icon"><Icon name="database" size={21} /></span><div><span class="muted small">Almacenamiento</span><strong class="text-value">SoupDB</strong></div><span class="badge personal">vía rsoup</span></div>
      </div>

      <div class="resource-layout" class:with-aside={view === 'library' || view === 'history'}>
        <div class="resource-main">
          <div class="toolbar resource-toolbar">
            <div class="search-field"><Icon name="search" size={18} /><label class="sr-only" for="resource-search">Buscar en {titles[view]}</label><input id="resource-search" bind:value={search} placeholder={view === 'history' ? 'Buscar una conversación…' : view === 'collections' ? 'Buscar una colección…' : 'Buscar por título, autor o palabra clave…'} /></div>
            <label class="sr-only" for="filter-status">Filtrar estado</label><select id="filter-status" class="input filter-select" bind:value={statusFilter}><option value="all">Todos los estados</option><option value="ready">Listos para consultar</option><option value="processing">En procesamiento</option><option value="error">Con errores</option></select>
            <div class="view-switch" aria-label="Presentación"><button class:active={layout === 'list'} onclick={() => layout = 'list'} aria-label="Vista de lista" aria-pressed={layout === 'list'}><Icon name="list" size={18} /></button><button class:active={layout === 'grid'} onclick={() => layout = 'grid'} aria-label="Vista de tarjetas" aria-pressed={layout === 'grid'}><Icon name="grid" size={18} /></button></div>
          </div>
          <div class="resource-card card" aria-busy={loading}>
            <div class="resource-card-heading"><h2>{view === 'history' ? 'Tus conversaciones' : view === 'collections' ? 'Tus proyectos' : 'Tu catálogo académico'}</h2><span class="chip">Solo personal</span></div>
            {#if layout === 'list' && (view === 'documents' || view === 'library')}<div class="table-placeholder" aria-hidden="true"><span>DOCUMENTO</span><span>ESTADO</span><span>AÑADIDO</span></div>{/if}
            <div class="empty-state">
              <div class="empty-art"><div class="paper-shape"></div><span><Icon name={view === 'history' ? 'history' : view === 'collections' ? 'folder' : 'file-text'} size={35} /></span></div>
              <span class="eyebrow">{loading ? 'CONECTANDO CON TU ESPACIO' : 'TODO EMPIEZA CON TUS FUENTES'}</span>
              <h2>{loading ? 'Consultando el servidor…' : view === 'history' ? 'Tu investigación deja un camino' : view === 'collections' ? 'Un lugar para cada proyecto' : 'Tus próximas lecturas, aquí'}</h2>
              <p>{view === 'history' ? 'Cuando el servicio documental esté conectado, aquí podrás retomar conversaciones y revisar sus fuentes.' : view === 'collections' ? 'Agrupa documentos por tema, curso o proyecto. La creación estará disponible al conectar el esquema documental.' : 'El catálogo todavía no está conectado. Cuando se habilite la ingestión, podrás incorporar y consultar tus documentos.'}</p>
              {#if search || statusFilter !== 'all'}<p class="fine-print">Los filtros se aplicarán cuando haya un catálogo disponible.</p><button class="button secondary" onclick={() => { search = ''; statusFilter = 'all'; }}>Limpiar filtros</button>{:else}<button class="button secondary" onclick={() => modal = 'connection'}><Icon name="settings" size={16} /> Revisar conexión</button>{/if}
            </div>
            <div class="resource-footer"><Icon name="info" size={15} /><span>Esquema e ingestión pendientes · No hay datos de demostración.</span></div>
          </div>
          {#if resourceError}<div class="api-notice" role="status"><Icon name="info" size={18} /><div><strong>No se pudo cargar {view === 'collections' ? 'las colecciones' : view === 'history' ? 'el historial' : 'el catálogo'}</strong><p>{resourceError}</p></div><button class="button ghost compact" disabled={loading} onclick={() => void loadResource(view)}>Reintentar</button></div>{/if}
        </div>
        {#if view === 'library' || view === 'history'}<aside class="context-card card"><span class="eyebrow">{view === 'history' ? 'CONTEXTO PRESERVADO' : 'DETALLES DEL DOCUMENTO'}</span><div class="context-icon"><Icon name={view === 'history' ? 'history' : 'file-search'} size={32} /></div><h3>{view === 'history' ? 'Retoma el hilo' : 'Una mirada más cerca'}</h3><p class="muted">{view === 'history' ? 'Selecciona una conversación para revisar sus mensajes y fuentes originales cuando el historial esté conectado.' : 'Selecciona un documento para consultar su información, estado de procesamiento y fuentes disponibles.'}</p><div class="context-bottom"><Icon name="shield" size={17} /><p>Las fuentes y citas se mostrarán únicamente cuando provengan de documentos reales.</p></div></aside>{/if}
      </div>
    </section>
  {/if}
</Shell>

{#if modal}
  <div class="modal-backdrop" role="presentation" onclick={(event) => { if (event.target === event.currentTarget) modal = null; }}>
    <div class="modal" role="dialog" aria-modal="true" aria-labelledby="modal-title" tabindex="-1" onkeydown={(event) => { if (event.key === 'Escape') modal = null; }} use:focusDialog>
      <div class="modal-header"><span class="eyebrow">CUYLIMEÑO · ESPACIO PERSONAL</span><button class="button ghost compact" onclick={() => modal = null} aria-label="Cerrar diálogo"><Icon name="x" size={20} /></button></div>
      <h2 id="modal-title">{modal === 'connection' ? 'Estado de tu espacio' : modal === 'upload' ? 'Añadir documentos académicos' : modal === 'collection' ? 'Nueva colección' : 'Fuentes de la consulta'}</h2>
      {#if modal === 'connection'}
        <p class="muted">El servidor Rust comprueba la conexión mediante el driver rsoup.</p>
        <dl class="connection-details"><div><dt>Servidor Rust</dt><dd>{capabilities ? 'Disponible' : 'Sin respuesta'}</dd></div><div><dt>SoupDB / rsoup</dt><dd>{connection === 'connected' ? 'Conectado' : connection === 'checking' ? 'Comprobando…' : 'Sin conexión'}</dd></div><div><dt>Esquema documental</dt><dd>Pendiente</dd></div><div><dt>Ingestión de PDF</dt><dd>Pendiente</dd></div><div><dt>GroqCloud</dt><dd>{capabilities?.groq_configured ? 'Configurado en el servidor' : 'Pendiente de configuración'}</dd></div>{#if capabilities?.groq_model}<div><dt>Modelo configurado</dt><dd>{capabilities.groq_model}</dd></div>{/if}</dl>
        {#if connectionError}<p class="api-notice" role="status">{connectionError}</p>{/if}
        <p class="fine-print">La clave de GroqCloud se configura exclusivamente en el servidor. La generación necesita fuentes documentales reales.</p>
        <div class="modal-actions"><button class="button secondary" onclick={() => modal = null}>Cerrar</button><button class="button primary" disabled={refreshing} onclick={() => void refreshConnection()}><Icon name="refresh" size={16} />{refreshing ? 'Comprobando…' : 'Comprobar conexión'}</button></div>
      {:else if modal === 'upload'}
        <p class="muted">Tus documentos se incorporarán a tu biblioteca personal.</p><div class="upload-zone"><Icon name="upload" size={35} /><h3>Ingestión de PDF pendiente</h3><p>No se seleccionarán ni enviarán archivos hasta conectar el servicio de ingestión.</p><button class="button secondary" disabled>Seleccionar PDF</button></div><div class="info-banner"><Icon name="info" size={18} /><span>{unavailable} No se han subido archivos.</span></div><div class="modal-actions"><button class="button primary" onclick={() => modal = null}>Entendido</button></div>
      {:else if modal === 'collection'}
        <p class="muted">Prepara el nombre de tu proyecto. Su persistencia requiere el esquema documental.</p><label class="field-label" for="collection-name">Nombre de la colección</label><input class="input full-width" id="collection-name" bind:value={collectionName} maxlength="120" placeholder="Por ejemplo, revisión de literatura" /><p class="fine-print">Borrador temporal. No se guardará ni enviará al servidor.</p><div class="info-banner"><Icon name="info" size={18} /><span>La creación de colecciones estará disponible cuando se conecte el esquema.</span></div><div class="modal-actions"><button class="button secondary" onclick={() => modal = null}>Cerrar</button><button class="button primary" disabled>Crear colección</button></div>
      {:else}
        <p class="muted">Elige los documentos que darán contexto a tu pregunta.</p><div class="source-empty"><Icon name="file-search" size={36} /><h3>No hay fuentes conectadas</h3><p>La selección estará disponible cuando el catálogo y la ingestión estén configurados.</p></div><div class="modal-actions"><button class="button secondary" onclick={() => modal = null}>Cerrar</button><button class="button primary" onclick={() => { modal = null; navigate('documents'); }}>Ir a mis documentos</button></div>
      {/if}
    </div>
  </div>
{/if}

<script lang="ts" module>
  function focusDialog(node: HTMLElement) {
    const previous = document.activeElement as HTMLElement | null;
    node.focus();
    const trap = (event: KeyboardEvent) => {
      if (event.key !== 'Tab') return;
      const focusable = Array.from(node.querySelectorAll<HTMLElement>('button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), [tabindex="0"]'));
      const first = focusable[0]; const last = focusable.at(-1);
      if (event.shiftKey && (document.activeElement === first || document.activeElement === node)) { event.preventDefault(); last?.focus(); }
      else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus(); }
    };
    node.addEventListener('keydown', trap);
    return { destroy() { node.removeEventListener('keydown', trap); previous?.focus(); } };
  }
</script>

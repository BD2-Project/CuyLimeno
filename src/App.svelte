<script lang="ts">
  import { onMount, tick } from 'svelte';
  import Shell from './lib/components/Shell.svelte';
  import Icon from './lib/components/Icon.svelte';
  import PdfViewer from './lib/components/PdfViewer.svelte';
  import { renderMarkdown } from './lib/markdown';
  import {
    ApiError, deleteConversation, deleteDocument, documentFilePageUrl, documentFileUrl,
    getConversation, listConversations, listDocuments, listModels, renameConversation, request,
    saveModel, streamChat, uploadDocument, viewFromHash, unavailable,
    type View, type Capabilities, type Document, type DocumentStatus, type Conversation,
    type Message, type Citation, type ModelInfo,
  } from './lib/api';

  const CONVERSATIONS_PAGE = 20;
  const MAX_MESSAGE_CHARS = 12_000;
  const statusLabels: Record<DocumentStatus, string> = { pending: 'En cola', processing: 'Procesando', ready: 'Listo', failed: 'Error' };
  const statusBadges: Record<DocumentStatus, string> = { pending: 'processing', processing: 'processing', ready: 'ready', failed: 'error' };

  let view = $state<View>(viewFromHash(window.location.hash));
  let capabilities = $state<Capabilities | null>(null);
  let connection = $state<'checking' | 'connected' | 'disconnected'>('checking');
  let connectionError = $state('');
  let resourceError = $state('');
  let schemaPending = $state(false);
  let loading = $state(false);
  let refreshing = $state(false);
  let search = $state('');
  let statusFilter = $state('all');
  let layout = $state<'grid' | 'list'>('list');
  let draft = $state('');
  let modal = $state<'upload' | 'replace-draft' | 'document' | 'rename' | null>(null);
  let suggestedDraft = $state('');
  let notice = $state('');
  let requestId = 0;

  let documents = $state<Document[]>([]);
  let conversations = $state<Conversation[]>([]);
  let conversationOffset = 0;
  let hasMoreConversations = $state(false);
  let loadingConversations = $state(false);
  let selectedDocumentIds = $state<string[]>([]);
  let messages = $state<Message[]>([]);
  let conversationId = $state<string | null>(null);
  let visibleMessageIds = $state<string[]>([]);
  let selectedCitation = $state<Citation | null>(null);
  let selectedDocument = $state<Document | null>(null);
  let documentExcerpt = $state('');
  let pdfPage = $state(1);
  let inspectorTab = $state<'evidence' | 'sources'>('evidence');
  let streaming = $state(false);
  let uploading = $state(false);
  let uploadError = $state('');
  let chatError = $state('');
  let lastQuestion = $state('');
  let renameTarget = $state<Conversation | null>(null);
  let renameTitle = $state('');
  let availableModels = $state<ModelInfo[]>([]);
  let modelsLoading = $state(false);
  let modelsError = $state('');
  let selectedModel = $state('');
  let savingModel = $state(false);
  let chatScrollEl = $state<HTMLDivElement>();
  let stickToBottom = true;
  let abortController: AbortController | null = null;
  let pollTimer: ReturnType<typeof setTimeout> | null = null;

  async function refreshConnection() {
    if (refreshing) return;
    refreshing = true;
    connection = 'checking';
    const [caps, health] = await Promise.allSettled([
      request<Capabilities>('/capabilities'), request<{ database: string }>('/health'),
    ]);
    capabilities = caps.status === 'fulfilled' ? caps.value : null;
    connection = health.status === 'fulfilled' && health.value.database === 'connected' ? 'connected' : 'disconnected';
    connectionError = health.status === 'rejected' ? String((health.reason as Error).message) : '';
    refreshing = false;
    if (capabilities?.schema_configured) {
      void Promise.allSettled([loadDocuments(), loadConversations()]);
    }
  }

  async function loadDocuments() {
    documents = await listDocuments();
  }

  async function loadConversations(reset = true) {
    if (reset) conversationOffset = 0;
    const page = await listConversations(conversationOffset, CONVERSATIONS_PAGE);
    conversations = reset ? page : [...conversations, ...page];
    conversationOffset += page.length;
    hasMoreConversations = page.length === CONVERSATIONS_PAGE;
  }

  async function loadMoreConversations() {
    if (loadingConversations || !hasMoreConversations) return;
    loadingConversations = true;
    try { await loadConversations(false); } catch (error) {
      notice = error instanceof Error ? error.message : 'No se pudieron cargar más conversaciones.';
    } finally { loadingConversations = false; }
  }

  async function loadResource(target: View) {
    const id = ++requestId;
    resourceError = '';
    schemaPending = false;
    if (!capabilities) return;
    if (!capabilities.schema_configured) {
      schemaPending = true;
      resourceError = 'El esquema documental está pendiente: arranca el motor SoupDB y vuelve a comprobar la conexión.';
      return;
    }
    if (target === 'settings') {
      await loadModels();
      return;
    }
    const needsLoading = target === 'documents';
    if (needsLoading) loading = true;
    try {
      if (target === 'documents') await loadDocuments();
    } catch (error) {
      if (id === requestId) {
        schemaPending = error instanceof ApiError && error.code === 'DOCUMENT_SCHEMA_NOT_CONFIGURED';
        resourceError = error instanceof Error ? error.message : 'No se pudo cargar el recurso.';
      }
    } finally { if (id === requestId && needsLoading) loading = false; }
  }

  async function loadModels() {
    if (!capabilities?.groq_configured) return;
    modelsLoading = true;
    modelsError = '';
    try {
      availableModels = await listModels();
      const active = capabilities?.groq_model ?? '';
      if (!availableModels.some((model) => model.id === selectedModel)) {
        selectedModel = availableModels.some((model) => model.id === active)
          ? active
          : (availableModels[0]?.id ?? '');
      }
    } catch (error) {
      modelsError = error instanceof Error ? error.message : 'No se pudieron cargar los modelos disponibles.';
    } finally {
      modelsLoading = false;
    }
  }

  async function saveSelectedModel() {
    if (!selectedModel) return;
    savingModel = true;
    modelsError = '';
    notice = '';
    try {
      const saved = await saveModel(selectedModel);
      if (capabilities) {
        capabilities = {
          ...capabilities,
          groq_model: saved.id,
          groq_configured: true,
          generation_available: capabilities.schema_configured,
        };
      }
      notice = 'Modelo de generación actualizado.';
    } catch (error) {
      modelsError = error instanceof Error ? error.message : 'No se pudo cambiar el modelo.';
    } finally {
      savingModel = false;
    }
  }

  function navigate(target: View) {
    view = target;
    window.location.hash = `/${target}`;
    search = '';
    statusFilter = 'all';
    notice = '';
    void loadResource(target);
  }

  async function initialize() {
    await refreshConnection();
    await loadResource(view);
  }

  onMount(() => {
    void initialize();
    const handleHash = () => {
      const next = viewFromHash(window.location.hash);
      if (next !== view) {
        view = next; search = ''; statusFilter = 'all'; notice = '';
        void loadResource(next);
      }
    };
    window.addEventListener('hashchange', handleHash);
    return () => {
      window.removeEventListener('hashchange', handleHash);
      abortController?.abort();
      if (pollTimer) clearTimeout(pollTimer);
    };
  });

  $effect(() => {
    messages;
    void tick().then(() => {
      computeVisibleMessages();
      if (streaming) void scrollToBottom();
    });
  });

  function filteredDocuments() {
    const query = search.trim().toLowerCase();
    return documents.filter((doc) => {
      if (statusFilter !== 'all' && doc.status !== statusFilter) return false;
      if (!query) return true;
      return `${doc.title} ${doc.authors.join(' ')} ${doc.filename}`.toLowerCase().includes(query);
    });
  }

  function formatDate(value: string) {
    if (!value) return '';
    const date = new Date(value);
    if (Number.isNaN(date.getTime())) return value;
    return date.toLocaleDateString('es-PE', { day: '2-digit', month: 'short', year: 'numeric' });
  }

  function documentMeta(doc: Document) {
    return [doc.authors.join(', '), doc.year ? String(doc.year) : '', doc.page_count ? `${doc.page_count} pág.` : '']
      .filter(Boolean).join(' · ');
  }

  function activeConversationTitle() {
    return conversations.find((conversation) => conversation.id === conversationId)?.title ?? '';
  }

  function scopeCaption() {
    if (selectedDocumentIds.length) return `${selectedDocumentIds.length} fuente${selectedDocumentIds.length === 1 ? '' : 's'} seleccionada${selectedDocumentIds.length === 1 ? '' : 's'}`;
    return documents.length ? 'Biblioteca completa' : 'Sin documentos';
  }

  function evidenceCitations(): Citation[] {
    const seen = new Set<string>();
    const result: Citation[] = [];
    for (const message of messages) {
      if (!visibleMessageIds.includes(message.id)) continue;
      for (const citation of message.citations) {
        const key = `${citation.document_id}|${citation.page}|${citation.excerpt.slice(0, 48)}`;
        if (seen.has(key)) continue;
        seen.add(key);
        result.push(citation);
      }
    }
    return result;
  }

  function computeVisibleMessages() {
    if (!chatScrollEl) return;
    const container = chatScrollEl.getBoundingClientRect();
    const visible: string[] = [];
    chatScrollEl.querySelectorAll<HTMLElement>('[data-message-id]').forEach((element) => {
      const rect = element.getBoundingClientRect();
      if (rect.bottom > container.top + 1 && rect.top < container.bottom - 1) {
        const id = element.dataset.messageId;
        if (id) visible.push(id);
      }
    });
    visibleMessageIds = visible;
  }

  async function scrollToBottom(force = false) {
    await tick();
    if (!chatScrollEl) return;
    if (force || stickToBottom) {
      chatScrollEl.scrollTop = chatScrollEl.scrollHeight;
    }
  }

  function handleChatScroll() {
    if (!chatScrollEl) return;
    stickToBottom = chatScrollEl.scrollHeight - chatScrollEl.scrollTop - chatScrollEl.clientHeight < 80;
    computeVisibleMessages();
  }

  function newConversation() {
    conversationId = null;
    messages = [];
    visibleMessageIds = [];
    selectedCitation = null;
    chatError = '';
    inspectorTab = 'evidence';
    selectedDocumentIds = [];
    navigate('chat');
  }

  async function openConversation(id: string) {
    try {
      const conversation = await getConversation(id);
      conversationId = conversation.id;
      messages = conversation.messages;
      selectedDocumentIds = conversation.document_ids;
      selectedCitation = null;
      visibleMessageIds = [];
      chatError = '';
      inspectorTab = 'evidence';
      navigate('chat');
      await scrollToBottom(true);
      computeVisibleMessages();
    } catch (error) {
      notice = error instanceof Error ? error.message : 'No se pudo abrir la conversación.';
    }
  }

  function beginRename(conversation: Conversation) {
    renameTarget = conversation;
    renameTitle = conversation.title;
    modal = 'rename';
  }

  async function submitRename() {
    const title = renameTitle.trim();
    if (!renameTarget || !title) return;
    try {
      const updated = await renameConversation(renameTarget.id, title);
      conversations = conversations.map((conversation) => conversation.id === updated.id
        ? { ...conversation, title: updated.title }
        : conversation);
      modal = null;
      notice = 'Conversación renombrada.';
    } catch (error) {
      notice = error instanceof Error ? error.message : 'No se pudo renombrar la conversación.';
    }
  }

  async function removeConversation(conversation: Conversation) {
    if (!window.confirm(`¿Eliminar la conversación «${conversation.title}»?`)) return;
    try {
      await deleteConversation(conversation.id);
      conversations = conversations.filter((value) => value.id !== conversation.id);
      if (conversationId === conversation.id) newConversation();
      notice = 'Conversación eliminada.';
    } catch (error) {
      notice = error instanceof Error ? error.message : 'No se pudo eliminar la conversación.';
    }
  }

  function toggleSource(id: string) {
    selectedDocumentIds = selectedDocumentIds.includes(id)
      ? selectedDocumentIds.filter((value) => value !== id)
      : [...selectedDocumentIds, id];
  }

  function useSuggestion(suggestion: string) {
    if (!draft.trim()) { draft = suggestion; return; }
    suggestedDraft = suggestion;
    modal = 'replace-draft';
  }

  async function copyDraft() {
    try { await navigator.clipboard.writeText(draft); notice = 'Borrador copiado.'; }
    catch { notice = 'No se pudo acceder al portapapeles. Puedes seleccionar y copiar el texto.'; }
  }

  async function sendMessage() {
    const text = draft.trim();
    if (!text || streaming) return;
    if (!capabilities?.generation_available) {
      notice = 'La generación no está disponible en este momento.';
      return;
    }
    draft = '';
    notice = '';
    resourceError = '';
    chatError = '';
    lastQuestion = text;
    streaming = true;
    stickToBottom = true;
    selectedCitation = null;
    const now = new Date().toISOString();
    const userMessage: Message = { id: `local-user-${Date.now()}`, role: 'user', content: text, citations: [], created_at: now };
    const assistantId = `local-assistant-${Date.now()}`;
    const assistantMessage: Message = { id: assistantId, role: 'assistant', content: '', citations: [], created_at: now };
    messages = [...messages, userMessage, assistantMessage];
    void scrollToBottom(true);
    let answer = '';
    abortController = new AbortController();
    try {
      await streamChat({
        conversation_id: conversationId,
        message: text,
        document_ids: selectedDocumentIds,
        collection_ids: [],
      }, {
        onMeta: (meta) => { conversationId = meta.conversation_id; },
        onDelta: (delta) => { answer += delta; updateMessage(assistantId, answer); },
        onCitations: (citations) => { updateMessage(assistantId, answer, citations); },
        onError: (error) => { chatError = error.message; },
      }, abortController.signal);
      if (answer) void loadConversations(true).catch(() => undefined);
    } catch (error) {
      if (error instanceof DOMException && error.name === 'AbortError') {
        if (answer) notice = 'Consulta detenida.';
      } else {
        chatError = error instanceof Error ? error.message : 'No se pudo completar la consulta.';
      }
      if (!answer) messages = messages.filter((message) => message.id !== assistantId);
    } finally {
      streaming = false;
      abortController = null;
    }
    if (!answer) messages = messages.filter((message) => message.id !== assistantId);
  }

  function updateMessage(id: string, content: string, citations?: Citation[]) {
    messages = messages.map((message) => message.id === id
      ? { ...message, content, citations: citations ?? message.citations }
      : message);
  }

  function stopStreaming() {
    abortController?.abort();
    streaming = false;
  }

  function handleComposerKeydown(event: KeyboardEvent) {
    if (event.key !== 'Enter') return;
    if (event.metaKey || event.ctrlKey) {
      event.preventDefault();
      insertNewline(event.currentTarget as HTMLTextAreaElement);
      return;
    }
    if (event.shiftKey || event.altKey) return;
    if (!draft.trim() || streaming || !capabilities?.generation_available) return;
    event.preventDefault();
    void sendMessage();
  }

  function insertNewline(textarea: HTMLTextAreaElement) {
    const start = textarea.selectionStart ?? draft.length;
    const end = textarea.selectionEnd ?? start;
    draft = `${draft.slice(0, start)}\n${draft.slice(end)}`;
    void tick().then(() => {
      textarea.selectionStart = start + 1;
      textarea.selectionEnd = start + 1;
    });
  }

  function retryLast() {
    if (!lastQuestion) return;
    draft = lastQuestion;
    chatError = '';
    void sendMessage();
  }

  function openCitation(citation: Citation) {
    selectedCitation = citation;
    openDocumentById(citation.document_id, { page: citation.page, excerpt: citation.excerpt });
  }

  function openDocumentById(documentId: string, highlight: { page: number; excerpt: string } | null = null) {
    const doc = documents.find((value) => value.id === documentId);
    if (!doc) {
      notice = 'El documento de esta cita ya no está en tu biblioteca.';
      return;
    }
    openDocument(doc, highlight);
  }

  function openDocument(doc: Document, highlight: { page: number; excerpt: string } | null = null) {
    selectedDocument = doc;
    pdfPage = highlight?.page ?? 1;
    documentExcerpt = highlight?.excerpt ?? '';
    modal = 'document';
  }

  function useDocumentInChat(doc: Document) {
    if (!selectedDocumentIds.includes(doc.id)) {
      selectedDocumentIds = [...selectedDocumentIds, doc.id];
    }
    inspectorTab = 'sources';
    modal = null;
    navigate('chat');
  }

  async function removeDocument(doc: Document) {
    if (!window.confirm(`¿Eliminar «${doc.title}» de tu biblioteca?`)) return;
    try {
      await deleteDocument(doc.id);
      documents = documents.filter((value) => value.id !== doc.id);
      selectedDocumentIds = selectedDocumentIds.filter((value) => value !== doc.id);
      notice = 'Documento eliminado.';
    } catch (error) {
      notice = error instanceof Error ? error.message : 'No se pudo eliminar el documento.';
    }
  }

  async function handleFile(event: Event) {
    const input = event.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    if (!file) return;
    uploadError = '';
    if (!file.name.toLowerCase().endsWith('.pdf')) { uploadError = 'Solo se admiten archivos PDF.'; input.value = ''; return; }
    if (file.size > 50 * 1024 * 1024) { uploadError = 'El PDF supera el límite de 50 MB.'; input.value = ''; return; }
    uploading = true;
    try {
      const created = await uploadDocument(file);
      documents = [created, ...documents.filter((doc) => doc.id !== created.id)];
      uploading = false;
      modal = null;
      void navigate('documents');
      notice = 'Documento en cola de procesamiento.';
      scheduleStatusRefresh(0);
    } catch (error) {
      uploading = false;
      uploadError = error instanceof Error ? error.message : 'No se pudo subir el PDF.';
    } finally { input.value = ''; }
  }

  function scheduleStatusRefresh(attempt: number) {
    if (pollTimer) clearTimeout(pollTimer);
    if (attempt > 40) return;
    if (!documents.some((doc) => doc.status === 'processing' || doc.status === 'pending')) return;
    pollTimer = setTimeout(async () => {
      try { documents = await listDocuments(); } catch { /* reintento silencioso */ }
      scheduleStatusRefresh(attempt + 1);
    }, 2500);
  }
</script>

<svelte:head><title>{view === 'documents' ? 'Mis documentos' : view === 'settings' ? 'Configuración' : 'Chat académico'} · CuyLimeño</title></svelte:head>

<Shell
  active={view}
  {conversations}
  activeConversationId={conversationId}
  {hasMoreConversations}
  loadingMoreConversations={loadingConversations}
  onNavigate={navigate}
  onNewConversation={newConversation}
  onSelectConversation={openConversation}
  onRenameConversation={beginRename}
  onDeleteConversation={removeConversation}
  onLoadMoreConversations={loadMoreConversations}
  onOpenSettings={() => navigate('settings')}
>
  {#if view === 'chat'}
    <div class="research-layout">
      <section class="research-chat" aria-label="Conversación académica">
        {#if messages.length}
          <div class="chat-heading compact">
            <div><span class="eyebrow">CONVERSACIÓN</span><h1>{activeConversationTitle() || 'Chat académico'}</h1></div>
            <span class="personal-mark"><Icon name="sparkles" size={22} /></span>
          </div>
        {:else}
          <div class="chat-heading"><div><span class="eyebrow">INVESTIGA CON EVIDENCIA</span><h1>Tu próxima idea empieza<br />con una buena pregunta.</h1><p class="muted">Conecta tus lecturas, contrasta ideas y vuelve siempre a la fuente.</p></div><span class="personal-mark"><Icon name="sparkles" size={22} /></span></div>
        {/if}

        <div class="chat-scroll" bind:this={chatScrollEl} onscroll={handleChatScroll}>
          {#if messages.length}
            <div class="chat-stream" aria-live="polite">
              {#each messages as message (message.id)}
                <div class="message-row" class:user={message.role === 'user'} data-message-id={message.id}>
                  <article class="message" class:user={message.role === 'user'}>
                    <div class="message-header">
                      {message.role === 'user' ? 'Tú' : 'CuyLimeño'}
                      {#if message.role === 'assistant' && !message.citations.length && !streaming}<span class="badge general">Respuesta general</span>{/if}
                      <small>{message.role === 'assistant' && streaming ? 'escribiendo…' : formatDate(message.created_at)}</small>
                    </div>
                    <div class="message-content markdown">{@html renderMarkdown(message.content || (message.role === 'assistant' && streaming ? '…' : ''))}</div>
                    {#if message.citations.length}
                      <div class="message-citations">
                        {#each message.citations as citation}
                          <button onclick={() => openCitation(citation)} title={citation.excerpt}>
                            <span class="citation">p. {citation.page}</span>{citation.document_title}
                          </button>
                        {/each}
                      </div>
                    {/if}
                  </article>
                </div>
              {/each}
            </div>
          {:else}
            <div class="chat-welcome card">
              <div class="welcome-icon"><Icon name="sparkles" size={25} /></div>
              <h2>Un espacio para pensar con tus documentos</h2>
              <p class="muted">Cada respuesta estará acompañada por fragmentos verificables de tus fuentes académicas.</p>
              <div class="suggestions">
                {#each ['Resume los aportes principales de mis lecturas', 'Compara las metodologías de los documentos', '¿Qué limitaciones señalan los autores?'] as suggestion}
                  <button onclick={() => useSuggestion(suggestion)}><Icon name="arrow-up-right" size={17} />{suggestion}</button>
                {/each}
              </div>
              <p class="fine-print">Las sugerencias solo preparan un borrador. No se envía información.</p>
            </div>
          {/if}
        </div>

        {#if chatError}
          <div class="api-notice" role="alert"><Icon name="warning" size={18} /><div><strong>No se pudo completar la consulta</strong><p>{chatError}</p></div><button class="button ghost compact" onclick={retryLast}>Reintentar</button></div>
        {/if}
        <div class="composer-wrap">
          <div class="question-composer">
            <label class="sr-only" for="question">Tu pregunta académica</label>
            <textarea id="question" bind:value={draft} placeholder="¿Qué quieres explorar en tus documentos?" rows="3" maxlength={MAX_MESSAGE_CHARS} title="Enter envía · ⌘/Ctrl+Enter inserta un salto de línea" onkeydown={handleComposerKeydown}></textarea>
            <div class="composer-actions"><span><Icon name="file-text" size={15} /> {documents.length ? `${documents.length} documento${documents.length === 1 ? '' : 's'} en tu biblioteca` : 'Sin fuentes disponibles'}</span><div>
              {#if streaming}<button class="button ghost compact" onclick={stopStreaming}><Icon name="close" size={16} /> Detener</button>{/if}
              <button class="button ghost compact" disabled={!draft.trim()} onclick={copyDraft} aria-label="Copiar borrador"><Icon name="copy" size={16} /></button>
              <button class="button primary compact" disabled={!draft.trim() || streaming || !capabilities?.generation_available} title={capabilities?.generation_available ? 'Enviar consulta' : 'Generación no disponible'} onclick={sendMessage}><Icon name="arrow-up" size={17} /> Consultar</button>
            </div></div>
          </div>
          {#if notice}<p class="notice" role="status">{notice}</p>{/if}
        </div>
      </section>

      <aside class="evidence-panel" aria-label="Inspector">
        <div class="inspector-tabs" role="tablist" aria-label="Panel derecho">
          <button class="chip" class:active={inspectorTab === 'evidence'} role="tab" aria-selected={inspectorTab === 'evidence'} onclick={() => inspectorTab = 'evidence'}>Evidencias y citas</button>
          <button class="chip" class:active={inspectorTab === 'sources'} role="tab" aria-selected={inspectorTab === 'sources'} onclick={() => inspectorTab = 'sources'}>Fuentes</button>
        </div>
        {#if inspectorTab === 'evidence'}
          <div class="inspector-title"><Icon name="quote" size={20} /><h2>Evidencias y citas</h2><span class="badge personal">Personal</span></div>
          <p class="muted small">Las citas de los mensajes que estás viendo.</p>
          {#if evidenceCitations().length}
            <div class="evidence-list">
              {#each evidenceCitations() as citation, index}
                <button class="evidence-item" class:selected={selectedCitation === citation} onclick={() => openCitation(citation)}>
                  <span><span class="citation">[{index + 1}]</span> <strong>{citation.document_title}</strong></span>
                  <small>página {citation.page}</small>
                  <p>{citation.excerpt}</p>
                </button>
              {/each}
            </div>
          {:else}
            <div class="evidence-empty"><div class="outlined-icon"><Icon name="file-search" size={30} /></div><h3>Cada afirmación, una fuente</h3><p>Las citas aparecen aquí a medida que lees la conversación.</p></div>
            <div class="evidence-guide"><span class="eyebrow">ASÍ FUNCIONA</span><ol><li>Incorpora tus documentos académicos.</li><li>Elige las fuentes de tu consulta.</li><li>Explora la respuesta y contrasta sus citas.</li></ol></div>
          {/if}
        {:else}
          <div class="inspector-title"><Icon name="document" size={20} /><h2>Fuentes</h2><span class="badge personal">{selectedDocumentIds.length || 'Todas'}</span></div>
          <p class="muted small">Marca o desmarca documentos; el cambio se aplica al instante.</p>
          {#if documents.length}
            <div class="source-list inspector-sources">
              {#each documents as doc (doc.id)}
                <label class="source-option" class:disabled={doc.status !== 'ready'}>
                  <input type="checkbox" checked={selectedDocumentIds.includes(doc.id)} disabled={doc.status !== 'ready'} onchange={() => toggleSource(doc.id)} />
                  <span><strong>{doc.title}</strong><small>{statusLabels[doc.status]} · {documentMeta(doc) || doc.filename}</small></span>
                </label>
              {/each}
            </div>
            <div class="source-actions">
              <span class="document-meta">{selectedDocumentIds.length ? `${selectedDocumentIds.length} seleccionada${selectedDocumentIds.length === 1 ? '' : 's'}` : 'Se usa la biblioteca completa'}</span>
              {#if selectedDocumentIds.length}<button class="button ghost compact" onclick={() => selectedDocumentIds = []}>Limpiar</button>{/if}
            </div>
          {:else}
            <div class="evidence-empty"><div class="outlined-icon"><Icon name="upload" size={30} /></div><h3>No hay documentos</h3><p>Sube un PDF para poder seleccionarlo como fuente.</p><button class="button secondary" onclick={() => modal = 'upload'}>Añadir documentos</button></div>
          {/if}
        {/if}
      </aside>
    </div>
  {:else if view === 'documents'}
    <section class="page-content">
      {#if notice}<p class="notice" role="status">{notice}</p>{/if}
      <div class="page-heading">
        <div><span class="eyebrow">TU ESPACIO DE INVESTIGACIÓN</span><h1>Mis documentos</h1><p class="muted">Organiza tus lecturas y prepara las fuentes de tu próxima investigación.</p></div>
        <button class="button primary" onclick={() => modal = 'upload'}><Icon name="upload" size={17} /> Añadir documentos</button>
      </div>

      <div class="overview-grid">
        <div class="overview-card"><span class="overview-icon"><Icon name="document" size={21} /></span><div><span class="muted small">Documentos personales</span><strong>{documents.length}</strong></div><span class="badge personal">Biblioteca</span></div>
        <div class="overview-card"><span class="overview-icon"><Icon name="quote" size={21} /></span><div><span class="muted small">Disponibles para consulta</span><strong>{documents.filter((doc) => doc.status === 'ready').length}</strong></div><span class="small muted">{capabilities?.retrieval_available ? 'Índice BM25 activo' : 'Sin datos conectados'}</span></div>
      </div>

      <div class="resource-layout">
        <div class="resource-main">
          <div class="toolbar resource-toolbar">
            <div class="search-field"><Icon name="search" size={18} /><label class="sr-only" for="resource-search">Buscar en Mis documentos</label><input id="resource-search" bind:value={search} placeholder="Buscar por título, autor o palabra clave…" /></div>
            <label class="sr-only" for="filter-status">Filtrar estado</label><select id="filter-status" class="input filter-select" bind:value={statusFilter}><option value="all">Todos los estados</option><option value="ready">Listos para consultar</option><option value="processing">En procesamiento</option><option value="failed">Con errores</option></select>
            <div class="view-switch" aria-label="Presentación"><button class:active={layout === 'list'} onclick={() => layout = 'list'} aria-label="Vista de lista" aria-pressed={layout === 'list'}><Icon name="list" size={18} /></button><button class:active={layout === 'grid'} onclick={() => layout = 'grid'} aria-label="Vista de tarjetas" aria-pressed={layout === 'grid'}><Icon name="grid" size={18} /></button></div>
          </div>
          <div class="resource-card card" aria-busy={loading}>
            <div class="resource-card-heading"><h2>Tu catálogo académico</h2><span class="chip">Solo personal</span></div>
            {#if loading}
              <div class="empty-state"><span class="eyebrow">CONECTANDO CON TU ESPACIO</span><h2>Consultando el servidor…</h2><p>Estamos recuperando tus datos desde SoupDB.</p></div>
            {:else if filteredDocuments().length}
              {#if layout === 'list'}
                <div class="table-scroll">
                  <table class="data-table">
                    <thead><tr><th scope="col">DOCUMENTO</th><th scope="col">ESTADO</th><th scope="col">AÑADIDO</th><th scope="col"><span class="sr-only">Acciones</span></th></tr></thead>
                    <tbody>
                      {#each filteredDocuments() as doc (doc.id)}
                        <tr>
                          <td class="document-title-cell">
                            <button class="document-open" onclick={() => openDocument(doc)} title="Ver el PDF"><strong>{doc.title}</strong></button>
                            <span class="document-meta">{documentMeta(doc) || doc.filename}</span>
                          </td>
                          <td><span class="badge {statusBadges[doc.status]}">{statusLabels[doc.status]}</span></td>
                          <td><span class="document-meta">{formatDate(doc.created_at)}</span></td>
                          <td class="document-actions">
                            <button class="button ghost compact" aria-label="Ver {doc.title}" onclick={() => openDocument(doc)}><Icon name="eye" size={15} /></button>
                            <button class="button ghost compact" aria-label="Eliminar {doc.title}" onclick={() => removeDocument(doc)}><Icon name="trash" size={15} /></button>
                          </td>
                        </tr>
                      {/each}
                    </tbody>
                  </table>
                </div>
              {:else}
                <div class="document-grid">
                  {#each filteredDocuments() as doc (doc.id)}
                    <article class="document-card">
                      <span class="badge {statusBadges[doc.status]}">{statusLabels[doc.status]}</span>
                      <button class="document-open" onclick={() => openDocument(doc)} title="Ver el PDF"><strong>{doc.title}</strong></button>
                      <span class="document-meta">{documentMeta(doc) || doc.filename}</span>
                      <div class="card-actions"><button class="button ghost compact" onclick={() => openDocument(doc)}><Icon name="eye" size={15} /> Ver PDF</button><button class="button ghost compact" aria-label="Eliminar {doc.title}" onclick={() => removeDocument(doc)}><Icon name="trash" size={15} /></button></div>
                    </article>
                  {/each}
                </div>
              {/if}
            {:else}
              <div class="empty-state">
                <div class="empty-art"><div class="paper-shape"></div><span><Icon name="file-text" size={35} /></span></div>
                <span class="eyebrow">{search || statusFilter !== 'all' ? 'SIN RESULTADOS' : 'TODO EMPIEZA CON TUS FUENTES'}</span>
                <h2>{search || statusFilter !== 'all' ? 'No hay coincidencias' : 'Tus próximas lecturas, aquí'}</h2>
                <p>{search || statusFilter !== 'all' ? 'Ajusta la búsqueda o los filtros para encontrar tus documentos.' : 'Sube tu primer paper en PDF. El texto se extrae por páginas y se indexa para citarlo con precisión.'}</p>
                {#if search || statusFilter !== 'all'}<button class="button secondary" onclick={() => { search = ''; statusFilter = 'all'; }}>Limpiar filtros</button>{:else}<button class="button secondary" onclick={() => modal = 'upload'}><Icon name="upload" size={16} /> Subir mi primer PDF</button>{/if}
              </div>
            {/if}
            <div class="resource-footer"><Icon name="info" size={15} /><span>{capabilities?.schema_configured ? 'Esquema documental e índice BM25 activos en SoupDB.' : 'Esquema e ingestión pendientes · No hay datos de demostración.'}</span></div>
          </div>
          {#if resourceError}<div class="api-notice" role="status"><Icon name="info" size={18} /><div><strong>{schemaPending ? 'Integración documental pendiente' : 'No se pudo cargar el catálogo'}</strong><p>{resourceError}</p></div>{#if !schemaPending}<button class="button ghost compact" disabled={loading} onclick={() => void loadResource(view)}>Reintentar</button>{/if}</div>{/if}
        </div>
      </div>
    </section>
  {:else}
    <section class="page-content">
      {#if notice}<p class="notice" role="status">{notice}</p>{/if}
      <div class="page-heading">
        <div><span class="eyebrow">CONFIGURACIÓN</span><h1>Mi espacio</h1><p class="muted">Estado de los servicios y modelo de generación.</p></div>
        <button class="button secondary" disabled={refreshing} onclick={() => void refreshConnection()}><Icon name="refresh" size={16} /> {refreshing ? 'Comprobando…' : 'Comprobar conexión'}</button>
      </div>
      <div class="settings-grid">
        <div class="card settings-card">
          <h2>Servicios</h2>
          <dl class="connection-details"><div><dt>Servidor Rust</dt><dd>{capabilities ? 'Disponible' : 'Sin respuesta'}</dd></div><div><dt>SoupDB / rsoup</dt><dd>{connection === 'connected' ? 'Conectado' : connection === 'checking' ? 'Comprobando…' : 'Sin conexión'}</dd></div><div><dt>Esquema documental</dt><dd>{capabilities?.schema_configured ? 'Configurado' : 'Pendiente'}</dd></div><div><dt>Ingestión de PDF</dt><dd>{capabilities?.ingestion_available ? 'Disponible' : 'Pendiente'}</dd></div><div><dt>Índice invertido</dt><dd>{capabilities?.retrieval_available ? 'Operativo' : 'Pendiente'}</dd></div><div><dt>GroqCloud</dt><dd>{capabilities?.groq_configured ? 'Configurado en el servidor' : 'Pendiente de configuración'}</dd></div></dl>
          {#if connectionError}<p class="api-notice" role="status">{connectionError}</p>{/if}
          <p class="fine-print">La generación utiliza el servicio configurado en el servidor.</p>
        </div>
        <div class="card settings-card">
          <h2>Modelo de generación</h2>
          {#if !capabilities?.groq_configured}
            <p class="muted">La selección de modelos no está disponible en este momento.</p>
          {:else}
            <label class="field-label" for="model-select">Modelo activo</label>
            <div class="select-wrap">
              <select id="model-select" class="input model-select" bind:value={selectedModel} disabled={modelsLoading}>
                {#if modelsLoading}<option value="">Cargando modelos…</option>{/if}
                {#each availableModels as model (model.id)}<option value={model.id}>{model.id}{model.context_window ? ` · ${model.context_window} tokens` : ''}</option>{/each}
              </select>
              <Icon name="chevron-down" size={16} />
            </div>
            <p class="fine-print">Recomendado: <strong>openai/gpt-oss-20b</strong> (rápido y de bajo consumo). Los modelos grandes responden mejor pero gastan más cuota.</p>
            {#if modelsError}<p class="api-notice" role="alert">{modelsError}</p>{/if}
            <div class="settings-actions">
              <button class="button outline compact" disabled={modelsLoading} onclick={() => void loadModels()}><Icon name="refresh" size={15} /> Recargar lista</button>
              <button class="button primary" disabled={!selectedModel || modelsLoading || savingModel || selectedModel === capabilities?.groq_model} onclick={saveSelectedModel}>{savingModel ? 'Guardando…' : 'Guardar modelo'}</button>
            </div>
          {/if}
        </div>
      </div>
    </section>
  {/if}
</Shell>

{#if modal}
  <div class="modal-backdrop" role="presentation" onclick={(event) => { if (event.target === event.currentTarget) modal = null; }}>
    <div class="modal" role="dialog" aria-modal="true" aria-labelledby="modal-title" tabindex="-1" onkeydown={(event) => { if (event.key === 'Escape') modal = null; }} use:focusDialog>
      <div class="modal-header"><span class="eyebrow">CUYLIMEÑO · ESPACIO PERSONAL</span><button class="button ghost compact" onclick={() => modal = null} aria-label="Cerrar diálogo"><Icon name="x" size={20} /></button></div>
      <h2 id="modal-title">{modal === 'replace-draft' ? '¿Reemplazar tu borrador?' : modal === 'upload' ? 'Añadir documentos académicos' : modal === 'rename' ? 'Renombrar conversación' : (selectedDocument?.title ?? 'Documento')}</h2>
      {#if modal === 'replace-draft'}
        <p class="muted">Ya tienes una pregunta escrita. Puedes conservarla o reemplazarla por esta sugerencia.</p>
        <blockquote class="info-banner">{suggestedDraft}</blockquote>
        <div class="modal-actions"><button class="button secondary" onclick={() => modal = null}>Conservar borrador</button><button class="button primary" onclick={() => { draft = suggestedDraft; modal = null; }}>Reemplazar borrador</button></div>
      {:else if modal === 'upload'}
        <p class="muted">Tus documentos se incorporarán a tu biblioteca personal.</p>
        {#if capabilities?.ingestion_available}
          <div class="upload-zone">
            <h3>Sube un paper en PDF</h3>
            <p>El texto se extrae por páginas y se indexa para citarlo con precisión. Tamaño máximo: 50 MB.</p>
            <label class="button secondary file-label">
              <input class="file-input" type="file" accept="application/pdf" disabled={uploading} onchange={handleFile} />
              <Icon name="upload" size={16} /> {uploading ? 'Subiendo…' : 'Seleccionar PDF'}
            </label>
          </div>
          {#if uploadError}<div class="api-notice" role="status"><Icon name="warning" size={18} /><div><strong>No se pudo subir el documento</strong><p>{uploadError}</p></div></div>{/if}
          <div class="modal-actions"><button class="button primary" onclick={() => modal = null} disabled={uploading}>Cerrar</button></div>
        {:else}
          <div class="upload-zone"><Icon name="upload" size={35} /><h3>Ingestión de PDF pendiente</h3><p>No se seleccionarán ni enviarán archivos hasta conectar el servicio de ingestión.</p><button class="button secondary" disabled>Seleccionar PDF</button></div><div class="info-banner"><Icon name="info" size={18} /><span>{unavailable} No se han subido archivos.</span></div><div class="modal-actions"><button class="button primary" onclick={() => modal = null}>Entendido</button></div>
        {/if}
      {:else if modal === 'rename'}
        <p class="muted">Ponle un nombre claro para encontrarla en la barra lateral.</p>
        <label class="field-label" for="rename-title">Título de la conversación</label>
        <input class="input full-width" id="rename-title" bind:value={renameTitle} maxlength="120" placeholder="Por ejemplo, revisión de literatura" />
        <div class="modal-actions"><button class="button secondary" onclick={() => modal = null}>Cerrar</button><button class="button primary" disabled={!renameTitle.trim()} onclick={submitRename}>Guardar</button></div>
      {:else if modal === 'document'}
        {#if selectedDocument}
          <p class="muted">{documentMeta(selectedDocument) || selectedDocument.filename}</p>
          <dl class="document-facts">
            <div><dt>Estado</dt><dd><span class="badge {statusBadges[selectedDocument.status]}">{statusLabels[selectedDocument.status]}</span></dd></div>
            <div><dt>Archivo</dt><dd>{selectedDocument.filename}</dd></div>
            <div><dt>Páginas</dt><dd>{selectedDocument.page_count ?? '—'}</dd></div>
            <div><dt>Añadido</dt><dd>{formatDate(selectedDocument.created_at)}</dd></div>
          </dl>
          <PdfViewer url={documentFileUrl(selectedDocument.id)} page={pdfPage} excerpt={documentExcerpt} />
          <div class="modal-actions">
            <a class="button secondary" href={documentFilePageUrl(selectedDocument.id, pdfPage)} target="_blank" rel="noopener">Abrir en pestaña nueva</a>
            <button class="button secondary" onclick={() => modal = null}>Cerrar</button>
            <button class="button primary" disabled={selectedDocument.status !== 'ready'} onclick={() => selectedDocument && useDocumentInChat(selectedDocument)}>Usar en el chat</button>
          </div>
        {/if}
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
      const focusable = Array.from(node.querySelectorAll<HTMLElement>('button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), a[href], [tabindex="0"], label.file-label'));
      const first = focusable[0]; const last = focusable.at(-1);
      if (event.shiftKey && (document.activeElement === first || document.activeElement === node)) { event.preventDefault(); last?.focus(); }
      else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus(); }
    };
    node.addEventListener('keydown', trap);
    return { destroy() { node.removeEventListener('keydown', trap); if (previous?.isConnected) previous.focus(); } };
  }
</script>

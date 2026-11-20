<script lang="ts">
  import type { Snippet } from 'svelte';
  import { onMount, tick } from 'svelte';
  import Icon from './Icon.svelte';
  import type { View, Conversation } from '../api';

  let {
    active,
    conversations = [],
    activeConversationId = null,
    hasMoreConversations = false,
    loadingMoreConversations = false,
    onNavigate,
    onNewConversation,
    onSelectConversation,
    onRenameConversation,
    onDeleteConversation,
    onLoadMoreConversations,
    onOpenSettings,
    children,
  }: {
    active: View;
    conversations?: Conversation[];
    activeConversationId?: string | null;
    hasMoreConversations?: boolean;
    loadingMoreConversations?: boolean;
    onNavigate: (view: View) => void;
    onNewConversation: () => void;
    onSelectConversation: (id: string) => void;
    onRenameConversation: (conversation: Conversation) => void;
    onDeleteConversation: (conversation: Conversation) => void;
    onLoadMoreConversations: () => void;
    onOpenSettings: () => void;
    children: Snippet;
  } = $props();

  let mobileOpen = $state(false);
  let mobileViewport = $state(false);
  let menuId = $state<string | null>(null);
  let menuConversation = $state<Conversation | null>(null);
  let menuPosition = $state({ top: 0, left: 0 });
  let listEl = $state<HTMLDivElement>();
  let fadeTop = $state(false);
  let fadeBottom = $state(false);
  let menuButton: HTMLButtonElement;
  let sidebarElement: HTMLElement;

  onMount(() => {
    const query = window.matchMedia('(max-width: 700px)');
    const update = () => { mobileViewport = query.matches; if (!query.matches) mobileOpen = false; };
    update(); query.addEventListener('change', update);
    const closeMenus = () => closeConversationMenu();
    const escape = (event: KeyboardEvent) => { if (event.key === 'Escape') closeConversationMenu(); };
    const resize = () => { closeConversationMenu(); updateListFades(); };
    window.addEventListener('click', closeMenus);
    window.addEventListener('keydown', escape);
    window.addEventListener('resize', resize);
    return () => {
      query.removeEventListener('change', update);
      window.removeEventListener('click', closeMenus);
      window.removeEventListener('keydown', escape);
      window.removeEventListener('resize', resize);
    };
  });

  $effect(() => {
    conversations;
    void tick().then(updateListFades);
  });

  function closeSidebarOnMobile() { if (mobileViewport) mobileOpen = false; }

  function closeMenu() { mobileOpen = false; menuButton?.focus(); }

  async function toggleMenu() {
    if (mobileOpen) { closeMenu(); return; }
    mobileOpen = true;
    await tick();
    sidebarElement.querySelector<HTMLElement>('a[href], button')?.focus();
  }

  function handleKeyboard(event: KeyboardEvent) {
    if (!mobileOpen) return;
    if (event.key === 'Escape') { event.preventDefault(); closeMenu(); }
    if (event.key === 'Tab') {
      const elements = Array.from(sidebarElement.querySelectorAll<HTMLElement>('a[href], button:not(:disabled)'));
      const first = elements[0]; const last = elements.at(-1);
      if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last?.focus(); }
      else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus(); }
    }
  }

  function navigate(view: View) {
    onNavigate(view);
    closeSidebarOnMobile();
    if (mobileViewport) void tick().then(() => document.getElementById('main-content')?.focus());
  }

  function selectConversation(id: string) {
    onSelectConversation(id);
    closeConversationMenu();
    closeSidebarOnMobile();
    if (mobileViewport) void tick().then(() => document.getElementById('main-content')?.focus());
  }

  function toggleConversationMenu(event: MouseEvent, conversation: Conversation) {
    event.stopPropagation();
    if (menuId === conversation.id) { closeConversationMenu(); return; }
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    const left = Math.max(8, Math.min(window.innerWidth - 150, rect.right - 140));
    const top = rect.bottom + 96 > window.innerHeight
      ? Math.max(8, rect.top - 88)
      : rect.bottom + 4;
    menuId = conversation.id;
    menuConversation = conversation;
    menuPosition = { top, left };
  }

  function closeConversationMenu() {
    menuId = null;
    menuConversation = null;
  }

  function updateListFades() {
    if (!listEl) return;
    fadeTop = listEl.scrollTop > 4;
    fadeBottom = listEl.scrollTop + listEl.clientHeight < listEl.scrollHeight - 4;
  }

  function formatDate(value: string) {
    if (!value) return '';
    const date = new Date(value);
    if (Number.isNaN(date.getTime())) return '';
    return date.toLocaleDateString('es-PE', { day: '2-digit', month: 'short' });
  }
</script>

<svelte:window onkeydown={handleKeyboard} />
<a class="skip-link" href="#main-content" onclick={(event) => { event.preventDefault(); document.getElementById('main-content')?.focus(); }}>Saltar al contenido</a>
<div class="app-shell">
  {#if mobileOpen}<button class="sidebar-scrim" aria-label="Cerrar navegación" tabindex="-1" onclick={closeMenu}></button>{/if}
  <aside bind:this={sidebarElement} id="main-navigation" inert={mobileViewport && !mobileOpen} class:mobile-open={mobileOpen} class="sidebar" aria-label="Navegación principal">
    <a class="brand" href="#chat" onclick={(event) => { event.preventDefault(); navigate('chat'); }}>
      <span class="brand-mark"><Icon name="book" size={23} /></span>
      <span><strong>CuyLimeño<span class="brand-dot">.</span></strong><small>Tu espacio de investigación</small></span>
    </a>
    <button class="button new-conversation" onclick={onNewConversation}><Icon name="plus" size={18} /> Nueva conversación</button>
    <nav class="main-nav" aria-label="Secciones">
      <button class:active={active === 'documents'} aria-current={active === 'documents' ? 'page' : undefined} onclick={() => navigate('documents')}>
        <Icon name="document" size={19} /><span>Mis documentos</span>{#if active === 'documents'}<span class="nav-active-dot"></span>{/if}
      </button>
    </nav>

    <div class="nav-section-label">CONVERSACIONES</div>
    <div class="conversation-list" class:fade-top={fadeTop} class:fade-bottom={fadeBottom} bind:this={listEl} onscroll={() => { closeConversationMenu(); updateListFades(); }}>
      {#each conversations as conversation (conversation.id)}
        <div class="conversation-item" class:active={conversation.id === activeConversationId && active === 'chat'}>
          <button class="conversation-select" onclick={() => selectConversation(conversation.id)} title={conversation.title}>
            <span class="conversation-title">{conversation.title}</span>
            <small>{formatDate(conversation.updated_at)}</small>
          </button>
          <button class="conversation-menu" aria-label={`Opciones de ${conversation.title}`} aria-haspopup="menu" aria-expanded={menuId === conversation.id} onclick={(event) => toggleConversationMenu(event, conversation)}>
            <Icon name="more" size={16} />
          </button>
        </div>
      {:else}
        <p class="conversation-empty">Aún no hay conversaciones. Empieza con una pregunta.</p>
      {/each}
    </div>
    {#if hasMoreConversations}
      <button class="button ghost compact load-more" disabled={loadingMoreConversations} onclick={onLoadMoreConversations}>
        {loadingMoreConversations ? 'Cargando…' : 'Cargar más'}
      </button>
    {/if}

    <div class="sidebar-note"><span class="note-icon"><Icon name="sparkles" size={18} /></span><strong>De la lectura a las ideas.</strong><p>Reúne tus documentos y explora el conocimiento con sus fuentes a mano.</p></div>
    <div class="sidebar-footer">
      <button class="settings-open" onclick={onOpenSettings} aria-label="Abrir configuración de mi espacio">
        <span class="avatar">CL</span>
        <div>
          <strong>Mi espacio académico</strong>
        </div>
        <Icon name="settings" size={15} />
      </button>
    </div>
  </aside>
  <div class="main-shell">
    <button bind:this={menuButton} class="icon-button mobile-menu floating-menu" aria-label={mobileOpen ? 'Cerrar navegación' : 'Abrir navegación'} aria-expanded={mobileOpen} aria-controls="main-navigation" onclick={toggleMenu}><Icon name="menu" /></button>
    <main id="main-content" tabindex="-1">{@render children()}</main>
  </div>
</div>

{#if menuConversation}
  <div class="conversation-popover floating-popover" style:top={`${menuPosition.top}px`} style:left={`${menuPosition.left}px`}>
    <button onclick={() => { const conversation = menuConversation; closeConversationMenu(); if (conversation) onRenameConversation(conversation); }}><Icon name="document" size={14} /> Renombrar</button>
    <button class="danger" onclick={() => { const conversation = menuConversation; closeConversationMenu(); if (conversation) onDeleteConversation(conversation); }}><Icon name="trash" size={14} /> Eliminar</button>
  </div>
{/if}

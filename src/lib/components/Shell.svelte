<script lang="ts">
  import type { Snippet } from 'svelte';
  import { onMount, tick } from 'svelte';
  import Icon from './Icon.svelte';
  type View = 'chat' | 'library' | 'documents' | 'collections' | 'history';
  type Connection = 'checking' | 'connected' | 'disconnected';
  let { active, onNavigate, children, connection = 'disconnected' }: { active: View; onNavigate: (view: View) => void; children: Snippet; connection?: Connection } = $props();
  let mobileOpen = $state(false);
  let mobileViewport = $state(false);
  let menuButton: HTMLButtonElement;
  let sidebarElement: HTMLElement;
  onMount(() => {
    const query = window.matchMedia('(max-width: 700px)');
    const update = () => { mobileViewport = query.matches; if (!query.matches) mobileOpen = false; };
    update(); query.addEventListener('change', update);
    return () => query.removeEventListener('change', update);
  });
  function closeMenu() { mobileOpen = false; menuButton?.focus(); }
  async function toggleMenu() {
    if (mobileOpen) { closeMenu(); return; }
    mobileOpen = true;
    await tick();
    sidebarElement.querySelector<HTMLElement>('a[href]')?.focus();
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
  const navigation: { id: View; label: string; icon: string }[] = [
    { id: 'chat', label: 'Chat académico', icon: 'chat' },
    { id: 'library', label: 'Mi biblioteca', icon: 'library' },
    { id: 'documents', label: 'Mis documentos', icon: 'document' },
    { id: 'collections', label: 'Colecciones', icon: 'collection' },
    { id: 'history', label: 'Historial de consultas', icon: 'history' },
  ];
  function navigate(view: View) {
    onNavigate(view);
    mobileOpen = false;
    if (mobileViewport) void tick().then(() => document.getElementById('main-content')?.focus());
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
    <button class="button new-conversation" onclick={() => navigate('chat')}><Icon name="plus" size={18} /> Nueva conversación</button>
    <div class="nav-section-label">ESPACIO PERSONAL</div>
    <nav class="main-nav">
      {#each navigation as item}
        <button class:active={active === item.id} aria-current={active === item.id ? 'page' : undefined} onclick={() => navigate(item.id)}>
          <Icon name={item.icon} size={19} /><span>{item.label}</span>{#if active === item.id}<span class="nav-active-dot"></span>{/if}
        </button>
      {/each}
    </nav>
    <div class="sidebar-note"><span class="note-icon"><Icon name="sparkles" size={18} /></span><strong>De la lectura a las ideas.</strong><p>Reúne tus documentos y explora el conocimiento con sus fuentes a mano.</p></div>
    <div class="sidebar-footer"><span class="avatar">CL</span><div><strong>Mi espacio académico</strong><small>Biblioteca personal · Local</small></div><Icon name="lock" size={15} /></div>
  </aside>
  <div class="main-shell">
    <header class="topbar">
      <div class="topbar-left"><button bind:this={menuButton} class="icon-button mobile-menu" aria-label={mobileOpen ? 'Cerrar navegación' : 'Abrir navegación'} aria-expanded={mobileOpen} aria-controls="main-navigation" onclick={toggleMenu}><Icon name="menu" /></button><span class="connection-status" class:connected={connection === 'connected'} class:checking={connection === 'checking'} role="status"><span></span>{connection === 'checking' ? 'COMPROBANDO CONEXIÓN' : connection === 'connected' ? 'SERVICIO CONECTADO' : 'SERVICIO SIN CONECTAR'}</span></div>
      <nav class="top-nav" aria-label="Accesos rápidos"><button class:current={active === 'chat'} onclick={() => navigate('chat')}>Consulta</button><button class:current={active === 'library' || active === 'documents'} onclick={() => navigate('library')}>Recursos</button><button class:current={active === 'collections'} onclick={() => navigate('collections')}>Colecciones</button></nav>
      <span class="topbar-divider"></span><span class="local-label"><Icon name="lock" size={13} /> Espacio local</span><span class="avatar top-avatar">CL</span>
    </header>
    <main id="main-content" tabindex="-1">{@render children()}</main>
  </div>
</div>

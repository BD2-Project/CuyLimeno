<script lang="ts">
  import * as pdfjs from 'pdfjs-dist';
  import type { PDFDocumentLoadingTask, PDFDocumentProxy } from 'pdfjs-dist';
  import workerUrl from 'pdfjs-dist/build/pdf.worker.min.mjs?url';
  import { computeHighlights, type HighlightRect } from '../pdf-highlight';

  pdfjs.GlobalWorkerOptions.workerSrc = workerUrl;

  let {
    url,
    page = 1,
    excerpt = '',
  }: { url: string; page?: number; excerpt?: string } = $props();

  let containerEl = $state<HTMLDivElement>();
  let canvasEl = $state<HTMLCanvasElement>();
  let loadingTask: PDFDocumentLoadingTask | null = null;
  let pdf = $state.raw<PDFDocumentProxy | null>(null);
  let textItems = $state.raw<unknown[]>([]);
  let viewport = $state.raw<{ transform: number[]; scale: number } | null>(null);
  let pageNumber = $state(1);
  let pageCount = $state(0);
  let highlights = $state<HighlightRect[]>([]);
  let loading = $state(true);
  let error = $state('');
  let renderToken = 0;
  let lastRequestedPage = 0;

  $effect(() => {
    const source = url;
    let cancelled = false;
    loading = true;
    error = '';
    highlights = [];
    pageNumber = 1;
    pageCount = 0;
    loadingTask = pdfjs.getDocument({ url: source });
    loadingTask.promise
      .then(async (document) => {
        if (cancelled) return;
        pdf = document;
        pageCount = document.numPages;
        pageNumber = Math.min(Math.max(1, page), pageCount);
        lastRequestedPage = pageNumber;
        await renderPage();
        loading = false;
      })
      .catch((cause: unknown) => {
        if (cancelled) return;
        error = cause instanceof Error ? cause.message : String(cause);
        loading = false;
      });
    return () => {
      cancelled = true;
      loadingTask?.destroy();
      loadingTask = null;
      pdf = null;
    };
  });

  $effect(() => {
    const requested = page;
    if (!pdf || !requested || requested === lastRequestedPage) return;
    lastRequestedPage = requested;
    const target = Math.min(Math.max(1, requested), pageCount || requested);
    pageNumber = target;
    void renderPage(target);
  });

  $effect(() => {
    const currentExcerpt = excerpt;
    if (viewport) highlights = computeHighlights(textItems as never, viewport, currentExcerpt);
  });

  async function renderPage(target = pageNumber) {
    if (!pdf || !canvasEl) return;
    const token = ++renderToken;
    try {
      const pdfPage = await pdf.getPage(target);
      if (token !== renderToken) return;
      const base = pdfPage.getViewport({ scale: 1 });
      const available = Math.max(320, (containerEl?.clientWidth ?? 720) - 26);
      const scale = Math.max(0.5, Math.min(2.4, available / base.width));
      const pageViewport = pdfPage.getViewport({ scale });
      const ratio = window.devicePixelRatio || 1;
      canvasEl.width = Math.floor(pageViewport.width * ratio);
      canvasEl.height = Math.floor(pageViewport.height * ratio);
      canvasEl.style.width = `${pageViewport.width}px`;
      canvasEl.style.height = `${pageViewport.height}px`;
      const context = canvasEl.getContext('2d');
      if (!context) return;
      context.setTransform(ratio, 0, 0, ratio, 0, 0);
      await pdfPage.render({
        canvas: canvasEl,
        canvasContext: context,
        viewport: pageViewport,
      }).promise;
      if (token !== renderToken) return;
      viewport = pageViewport;
      textItems = (await pdfPage.getTextContent()).items;
      highlights = computeHighlights(textItems as never, pageViewport, excerpt);
    } catch (cause) {
      if (token === renderToken) {
        error = cause instanceof Error ? cause.message : String(cause);
      }
    }
  }

  function previousPage() {
    if (pageNumber <= 1) return;
    pageNumber -= 1;
    void renderPage(pageNumber);
  }

  function nextPage() {
    if (pageNumber >= pageCount) return;
    pageNumber += 1;
    void renderPage(pageNumber);
  }
</script>

<div class="pdf-shell">
  <div class="pdf-toolbar">
    <button class="button ghost compact" disabled={pageNumber <= 1 || loading} onclick={previousPage}>‹ Anterior</button>
    <span class="document-meta">Página {pageNumber} de {pageCount || '…'}</span>
    <button class="button ghost compact" disabled={pageNumber >= pageCount || loading} onclick={nextPage}>Siguiente ›</button>
  </div>
  {#if error}
    <div class="api-notice" role="alert">
      <span><strong>No se pudo renderizar el PDF</strong><p>{error}</p></span>
    </div>
    <a class="button secondary" href={url} target="_blank" rel="noopener">Abrir PDF en una pestaña nueva</a>
  {:else}
    <div class="pdf-scroll" bind:this={containerEl}>
      {#if loading}<p class="document-meta">Cargando PDF…</p>{/if}
      <div class="pdf-page">
        <canvas bind:this={canvasEl}></canvas>
        {#each highlights as rect, index (index)}
          <span
            class="pdf-highlight"
            style:left={`${rect.left}px`}
            style:top={`${rect.top}px`}
            style:width={`${rect.width}px`}
            style:height={`${rect.height}px`}
          ></span>
        {/each}
      </div>
    </div>
  {/if}
</div>
{#if excerpt}
  <blockquote class="pdf-excerpt">«{excerpt}»</blockquote>
{/if}

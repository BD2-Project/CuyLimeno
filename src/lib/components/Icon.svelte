<script lang="ts">
  let { name, size = 20, class: className = '' }: { name: string; size?: number; class?: string } = $props();
  const paths: Record<string, string[]> = {
    'file-search': ['M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h6', 'M14 2v6h6V8l-6-6', 'M21 17a4 4 0 1 1-8 0 4 4 0 0 1 8 0', 'm20 20 3 3'],
    'arrow-up-right': ['M7 17 17 7', 'M7 7h10v10'],
    grid: ['M3 3h7v7H3z', 'M14 3h7v7h-7z', 'M3 14h7v7H3z', 'M14 14h7v7h-7z'],
    list: ['M8 6h13', 'M8 12h13', 'M8 18h13', 'M3 6h.01', 'M3 12h.01', 'M3 18h.01'],
    database: ['M21 5c0 2-4 3-9 3S3 7 3 5s4-3 9-3 9 1 9 3Z', 'M3 5v14c0 2 4 3 9 3s9-1 9-3V5', 'M3 12c0 2 4 3 9 3s9-1 9-3'],
    shield: ['M12 3 3 6v6c0 5 9 9 9 9s9-4 9-9V6Z', 'm8 12 3 3 5-6'],
    messages: ['M21 11a7 7 0 0 1-7 7H9l-5 3v-6a7 7 0 0 1-2-5 7 7 0 0 1 7-7h5a7 7 0 0 1 7 8Z', 'M7 8h9', 'M7 12h6'],
    chat: ['M21 11.5a8.4 8.4 0 0 1-.9 3.8 8.5 8.5 0 0 1-7.6 4.7 8.4 8.4 0 0 1-3.8-.9L3 21l1.9-5.7a8.4 8.4 0 0 1-.9-3.8 8.5 8.5 0 0 1 4.7-7.6 8.4 8.4 0 0 1 3.8-.9h.5a8.5 8.5 0 0 1 8 8v.5Z'],
    library: ['M4 4h5v16H4z', 'M9 4h5v16H9z', 'm15 5 4-1 4 15-4 1z'],
    document: ['M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8Z', 'M14 2v6h6', 'M8 13h8', 'M8 17h5'],
    collection: ['M20 21H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2v11a2 2 0 0 1-2 2Z'],
    history: ['M3 11a9 9 0 1 1 2.5 7', 'M3 4v7h7', 'M12 7v5l3 2'],
    plus: ['M12 5v14', 'M5 12h14'], search: ['M21 21l-4.3-4.3', 'M19 11a8 8 0 1 1-16 0 8 8 0 0 1 16 0'],
    arrow: ['M12 19V5', 'm5 12 7-7 7 7'], arrowRight: ['M5 12h14', 'm12 5 7 7-7 7'],
    chevron: ['m9 5 7 7-7 7'], chevronDown: ['m6 9 6 6 6-6'],
    close: ['M18 6 6 18', 'm6 6 12 12'], check: ['m5 12 4 4L19 6'],
    upload: ['M12 16V3', 'm7 8 5-5 5 5', 'M3 16v4a1 1 0 0 0 1 1h16a1 1 0 0 0 1-1v-4'],
    download: ['M12 3v13', 'm7 11 5 5 5-5', 'M3 17v4h18v-4'],
    copy: ['M9 9h12v12H9z', 'M5 15H3V3h12v2'],
    trash: ['M3 6h18', 'M9 6V3h6v3', 'm5 6 1 15h12l1-15', 'M10 10v7', 'M14 10v7'],
    book: ['M12 5v16', 'M12 5C8 2 4 3 2 4v16c4-2 7-1 10 1 3-2 6-3 10-1V4c-2-1-6-2-10 1Z'],
    sparkles: ['m12 3 2.5 6.5L21 12l-6.5 2.5L12 21l-2.5-6.5L3 12l6.5-2.5Z', 'M20 2v4', 'M18 4h4'],
    info: ['M12 11v6', 'M12 7h.01', 'M22 12a10 10 0 1 1-20 0 10 10 0 0 1 20 0'],
    warning: ['m12 3 10 18H2Z', 'M12 9v5', 'M12 17h.01'],
    settings: ['M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8', 'm9 3 1-1h4l1 3 3 2 3-1 2 4-2 2v3l2 2-2 4-3-1-3 2-1 3h-4l-1-3-3-2-3 1-2-4 2-2v-3l-2-2 2-4 3 1Z'],
    filter: ['M4 7h16', 'M7 12h10', 'M10 17h4'], more: ['M5 12h.01', 'M12 12h.01', 'M19 12h.01'],
    link: ['M10 13a5 5 0 0 0 7 0l3-3a5 5 0 0 0-7-7l-2 2', 'M14 11a5 5 0 0 0-7 0l-3 3a5 5 0 0 0 7 7l2-2'],
    menu: ['M3 6h18', 'M3 12h18', 'M3 18h18'],
    lock: ['M5 10h14v11H5z', 'M8 10V6a4 4 0 0 1 8 0v4'],
    globe: ['M22 12a10 10 0 1 1-20 0 10 10 0 0 1 20 0', 'M2 12h20', 'M12 2a20 20 0 0 1 0 20 20 20 0 0 1 0-20'],
    eye: ['M2 12s4-7 10-7 10 7 10 7-4 7-10 7S2 12 2 12', 'M15 12a3 3 0 1 1-6 0 3 3 0 0 1 6 0'],
    quote: ['M3 5h7v7H6c0 3 1 4 3 5l-2 3c-4-2-4-5-4-8Z', 'M14 5h7v7h-4c0 3 1 4 3 5l-2 3c-4-2-4-5-4-8Z'],
    refresh: ['M20 7a9 9 0 1 0 1 8', 'M20 2v5h-5'],
  };
  const aliases: Record<string, string> = { 'file-text': 'document', 'arrow-up': 'arrow', folder: 'collection', 'chevron-down': 'chevronDown', x: 'close' };
</script>

<svg xmlns="http://www.w3.org/2000/svg" width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round" class={className} aria-hidden="true">
  {#each paths[aliases[name] ?? name] ?? paths.document as path}<path d={path} />{/each}
</svg>

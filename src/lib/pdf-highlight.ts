export interface PdfTextItem {
  str: string;
  transform: number[];
  width: number;
  height: number;
}

export interface PdfViewport {
  transform: number[];
  scale: number;
}

export interface HighlightRect {
  left: number;
  top: number;
  width: number;
  height: number;
}

function multiply(m1: number[], m2: number[]): number[] {
  return [
    m1[0] * m2[0] + m1[2] * m2[1],
    m1[1] * m2[0] + m1[3] * m2[1],
    m1[0] * m2[2] + m1[2] * m2[3],
    m1[1] * m2[2] + m1[3] * m2[3],
    m1[0] * m2[4] + m1[2] * m2[5] + m1[4],
    m1[1] * m2[4] + m1[3] * m2[5] + m1[5],
  ];
}

function normalize(text: string): string {
  return text
    .toLowerCase()
    .normalize('NFD')
    .replace(/[\u0300-\u036f]/g, '')
    .replace(/[^a-z0-9]+/g, ' ')
    .trim();
}

/**
 * Calcula los rectángulos (en píxeles CSS de la página renderizada) que
 * corresponden al extracto citado. Ancla la búsqueda en la palabra más
 * distintiva del inicio del extracto y resalta los ítems contiguos que cubren
 * una longitud similar a la cita.
 */
export function computeHighlights(
  items: PdfTextItem[],
  viewport: PdfViewport,
  excerpt: string,
): HighlightRect[] {
  const words = normalize(excerpt)
    .split(' ')
    .filter((word) => word.length >= 4);
  if (!words.length) return [];
  const head = words.slice(0, 6);
  const anchor =
    words.find((word) => word.length >= 5) ??
    head.reduce((best, word) => (word.length > best.length ? word : best), head[0]);
  let start = items.findIndex((item) => normalize(item.str).includes(anchor));
  if (start < 0) {
    start = items.findIndex((item) => words.some((word) => normalize(item.str).includes(word)));
  }
  if (start < 0) return [];

  const targetLength = Math.min(normalize(excerpt).length, 600);
  const rects: HighlightRect[] = [];
  let consumed = 0;
  let lastTop: number | null = null;
  for (let index = start; index < items.length && consumed < targetLength && rects.length < 120; index++) {
    const item = items[index];
    if (!item.str.trim()) continue;
    const matrix = multiply(viewport.transform, item.transform);
    const fontHeight = Math.hypot(matrix[2], matrix[3]) || item.height * viewport.scale;
    if (lastTop !== null && Math.abs(matrix[5] - lastTop) > fontHeight * 8) break;
    lastTop = matrix[5];
    const left = matrix[4];
    const top = matrix[5] - fontHeight;
    const width = Math.max(item.width * viewport.scale, 1);
    if (fontHeight > 0 && width > 0) {
      rects.push({ left, top, width, height: fontHeight });
    }
    consumed += normalize(item.str).length + 1;
  }
  return rects;
}

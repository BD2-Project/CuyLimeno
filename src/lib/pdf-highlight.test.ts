import { describe, expect, it } from 'vitest';
import { computeHighlights } from './pdf-highlight';

const items = [
  { str: 'Fragmento', transform: [10, 0, 0, 10, 40, 720], width: 45, height: 10 },
  { str: 'citado verificable', transform: [10, 0, 0, 10, 40, 700], width: 90, height: 10 },
  { str: 'Otra linea distinta', transform: [10, 0, 0, 10, 40, 680], width: 110, height: 10 },
];
const viewport = { transform: [1, 0, 0, -1, 0, 800], scale: 1 };

describe('computeHighlights', () => {
  it('encuentra el fragmento citado y calcula rectángulos', () => {
    const rects = computeHighlights(items, viewport, 'Fragmento citado verificable sobre el motor');
    expect(rects.length).toBeGreaterThan(0);
    expect(rects[0].left).toBe(40);
    expect(rects[0].top).toBe(70);
    expect(rects[0].width).toBeGreaterThan(0);
    expect(rects[0].height).toBeGreaterThan(0);
  });

  it('devuelve vacío cuando el extracto no aparece en la página', () => {
    expect(computeHighlights(items, viewport, 'contenido inexistente totalmente')).toEqual([]);
  });

  it('ignora extractos sin palabras útiles', () => {
    expect(computeHighlights(items, viewport, 'de la el')).toEqual([]);
  });

  it('normaliza acentos y mayúsculas al buscar', () => {
    const rects = computeHighlights(items, viewport, 'FRAGMENTO CITADO VERIFICABLE');
    expect(rects.length).toBeGreaterThan(0);
  });
});

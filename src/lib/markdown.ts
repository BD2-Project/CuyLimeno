import DOMPurify from 'dompurify';
import { marked } from 'marked';

marked.setOptions({
  gfm: true,
  breaks: true,
});

/** Renderiza Markdown del chat y lo sanea antes de inyectarlo. */
export function renderMarkdown(text: string): string {
  const html = marked.parse(text ?? '', { async: false }) as string;
  return DOMPurify.sanitize(html, { USE_PROFILES: { html: true } });
}

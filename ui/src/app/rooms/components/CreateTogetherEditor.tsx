'use client';

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type {
  ChangeEvent,
  PointerEvent as ReactPointerEvent,
} from 'react';
import { clientErrorMessage } from '@/lib/errors';
import type { WsCreateCanvasStroke, WsCreateStateMessage } from '@/lib/watchPartyApi';

type Props = {
  createState: WsCreateStateMessage | null;
  canEdit: boolean;
  sendWs: (payload: Record<string, unknown>) => boolean;
  activeToolOverride?: ActiveTool;
};

type Point = { x: number; y: number };
type CanvasViewport = { scale: number; offsetX: number; offsetY: number };
type CanvasWheelEventLike = {
  deltaX: number;
  deltaY: number;
  clientX: number;
  clientY: number;
  ctrlKey: boolean;
  metaKey: boolean;
  preventDefault: () => void;
  stopPropagation?: () => void;
};

type ActiveTool = 'text' | 'canvas';
type TextFormat = 'plain' | 'markdown' | 'pdf_text';
type PageSize = 'a4' | 'letter' | 'legal' | 'a5' | 'tabloid';
type PageOrientation = 'portrait' | 'landscape';

type RichDocPage = {
  id: string;
  html: string;
};

type RichDocument = {
  version: 1;
  type: 'rich_doc';
  page_size: PageSize;
  page_orientation: PageOrientation;
  pages: RichDocPage[];
};

type TextToolbarState = {
  bold: boolean;
  italic: boolean;
  align: 'left' | 'center' | 'right' | 'justify';
  unorderedList: boolean;
  orderedList: boolean;
};

const MAX_DOC_NAME = 120;
const MAX_DOC_PAGES = 80;
const EMPTY_PAGE_HTML = '<p><br></p>';
const MIN_FONT_SIZE_PX = 8;
const MAX_FONT_SIZE_PX = 72;
const CANVAS_WIDTH = 1000;
const CANVAS_HEIGHT = 560;
const CANVAS_MIN_ZOOM = 0.01;
const CANVAS_MAX_ZOOM = 2;
const CANVAS_MAX_BYTES = 30 * 1024 * 1024;
const CANVAS_MAX_PAN_OFFSET = 2_000_000;
const CANVAS_WHEEL_ZOOM_SENSITIVITY = 0.0022;

const FONT_OPTIONS = [
  { label: 'Arial', css: 'Arial, sans-serif', command: 'Arial' },
  { label: 'Calibri', css: 'Calibri, Arial, sans-serif', command: 'Calibri' },
  {
    label: 'Times New Roman',
    css: '"Times New Roman", Times, serif',
    command: 'Times New Roman',
  },
  { label: 'Georgia', css: 'Georgia, serif', command: 'Georgia' },
  { label: 'Verdana', css: 'Verdana, Geneva, sans-serif', command: 'Verdana' },
  {
    label: 'Trebuchet MS',
    css: '"Trebuchet MS", Helvetica, sans-serif',
    command: 'Trebuchet MS',
  },
  { label: 'Courier New', css: '"Courier New", Courier, monospace', command: 'Courier New' },
] as const;

const FONT_SIZE_OPTIONS = [9, 10, 11, 12, 14, 15, 16, 18, 20, 24, 28, 32, 36, 48, 64, 72] as const;

const PAGE_SIZES: Record<PageSize, { label: string; widthPx: number; heightPx: number }> = {
  a4: { label: 'A4', widthPx: 794, heightPx: 1123 },
  letter: { label: 'Letter', widthPx: 816, heightPx: 1056 },
  legal: { label: 'Legal', widthPx: 816, heightPx: 1344 },
  a5: { label: 'A5', widthPx: 559, heightPx: 794 },
  tabloid: { label: 'Tabloid', widthPx: 1056, heightPx: 1632 },
};

const FIXED_PAGE_SIZE: PageSize = 'a4';
const DEFAULT_TOOLBAR_STATE: TextToolbarState = {
  bold: false,
  italic: false,
  align: 'left',
  unorderedList: false,
  orderedList: false,
};

function isPageOrientation(value: string): value is PageOrientation {
  return value === 'portrait' || value === 'landscape';
}

function sanitizeDocumentName(name: string): string {
  const trimmed = name.trim();
  if (!trimmed) return 'Untitled Document';
  return trimmed.slice(0, MAX_DOC_NAME);
}

function fileBaseName(fileName: string): string {
  const withoutExt = fileName.replace(/\.[^.]+$/, '');
  return sanitizeDocumentName(withoutExt);
}

function downloadBlob(blob: Blob, fileName: string) {
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = fileName;
  a.click();
  URL.revokeObjectURL(url);
}

function createPageId(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return crypto.randomUUID();
  }
  return `${Date.now()}-${Math.random().toString(36).slice(2, 10)}`;
}

function makeEmptyPage(): RichDocPage {
  return {
    id: createPageId(),
    html: EMPTY_PAGE_HTML,
  };
}

function escapeHtml(value: string): string {
  return value
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

function plainTextToPageHtml(value: string): string {
  const normalized = value.replace(/\r\n/g, '\n');
  const lines = normalized.split('\n');
  if (lines.length === 0) {
    return EMPTY_PAGE_HTML;
  }

  const html = lines
    .map((line) => {
      const escaped = escapeHtml(line);
      return escaped.length === 0 ? '<p><br></p>' : `<p>${escaped}</p>`;
    })
    .join('');

  return html || EMPTY_PAGE_HTML;
}

function normalizeFontFamilyKey(value: string): string {
  return value
    .replace(/["']/g, '')
    .replace(/\s*,\s*/g, ',')
    .replace(/\s+/g, ' ')
    .trim()
    .toLowerCase();
}

function coerceFontFamily(value: string): string | null {
  const normalized = normalizeFontFamilyKey(value);
  if (!normalized) return null;

  for (const option of FONT_OPTIONS) {
    const optionKey = normalizeFontFamilyKey(option.css);
    const labelKey = normalizeFontFamilyKey(option.label);
    const commandKey = normalizeFontFamilyKey(option.command);
    if (
      normalized === optionKey ||
      normalized === labelKey ||
      normalized === commandKey ||
      normalized.startsWith(`${commandKey},`)
    ) {
      return option.css;
    }
  }

  return null;
}

function sanitizeStyleValue(styleValue: string): string | null {
  const parts = styleValue
    .split(';')
    .map((part) => part.trim())
    .filter((part) => part.length > 0);

  const next: string[] = [];
  for (const part of parts) {
    const [rawProp, ...rawValueParts] = part.split(':');
    if (!rawProp || rawValueParts.length === 0) continue;
    const property = rawProp.trim().toLowerCase();
    const value = rawValueParts.join(':').trim();

    if (property === 'text-align') {
      const normalizedAlign = value.toLowerCase();
      if (['left', 'center', 'right', 'justify'].includes(normalizedAlign)) {
        next.push(`text-align:${normalizedAlign};`);
      }
      continue;
    }

    if (property === 'font-family') {
      const family = coerceFontFamily(value);
      if (family) {
        next.push(`font-family:${family};`);
      }
      continue;
    }

    if (property === 'font-size') {
      const match = value.match(/^(\d+(?:\.\d+)?)px$/i);
      if (!match) continue;
      const px = Number.parseFloat(match[1]);
      if (!Number.isFinite(px)) continue;
      const clamped = Math.round(clamp(px, MIN_FONT_SIZE_PX, MAX_FONT_SIZE_PX));
      next.push(`font-size:${clamped}px;`);
      continue;
    }
  }

  if (next.length === 0) return null;
  return next.join('');
}

function sanitizePageHtml(rawHtml: string): string {
  if (typeof window === 'undefined' || typeof DOMParser === 'undefined') {
    return rawHtml || EMPTY_PAGE_HTML;
  }

  const parser = new DOMParser();
  const doc = parser.parseFromString(`<div>${rawHtml}</div>`, 'text/html');
  const root = doc.body.firstElementChild as HTMLElement | null;
  if (!root) return EMPTY_PAGE_HTML;

  const allowedTags = new Set([
    'P',
    'DIV',
    'SPAN',
    'B',
    'STRONG',
    'I',
    'EM',
    'U',
    'BR',
    'UL',
    'OL',
    'LI',
    'H1',
    'H2',
    'H3',
    'H4',
    'H5',
    'H6',
    'BLOCKQUOTE',
  ]);

  const walk = (node: Node) => {
    if (node.nodeType === Node.ELEMENT_NODE) {
      const el = node as HTMLElement;
      if (!allowedTags.has(el.tagName)) {
        const parent = el.parentNode;
        if (parent) {
          while (el.firstChild) {
            parent.insertBefore(el.firstChild, el);
          }
          parent.removeChild(el);
        }
        return;
      }

      for (const attr of Array.from(el.attributes)) {
        const name = attr.name.toLowerCase();
        if (name.startsWith('on')) {
          el.removeAttribute(attr.name);
          continue;
        }

        if (name === 'style') {
          const sanitizedStyle = sanitizeStyleValue(attr.value);
          if (sanitizedStyle) {
            el.setAttribute('style', sanitizedStyle);
          } else {
            el.removeAttribute(attr.name);
          }
          continue;
        }

        el.removeAttribute(attr.name);
      }
    }

    for (const child of Array.from(node.childNodes)) {
      walk(child);
    }
  };

  walk(root);

  const cleaned = root.innerHTML.trim();
  return cleaned.length > 0 ? cleaned : EMPTY_PAGE_HTML;
}

function htmlToPlainText(html: string): string {
  if (typeof window === 'undefined' || typeof DOMParser === 'undefined') {
    return html;
  }
  const parser = new DOMParser();
  const doc = parser.parseFromString(`<div>${html}</div>`, 'text/html');
  return doc.body.textContent || '';
}

function normalizePages(input: RichDocPage[]): RichDocPage[] {
  const next = input
    .slice(0, MAX_DOC_PAGES)
    .map((page) => ({
      id: page.id || createPageId(),
      html: sanitizePageHtml(page.html || ''),
    }));

  if (next.length === 0) {
    return [makeEmptyPage()];
  }

  return next;
}

function serializeRichDocument(
  pages: RichDocPage[],
  pageSize: PageSize,
  pageOrientation: PageOrientation,
): string {
  const payload: RichDocument = {
    version: 1,
    type: 'rich_doc',
    page_size: pageSize,
    page_orientation: pageOrientation,
    pages: normalizePages(pages),
  };
  return JSON.stringify(payload);
}

function decodeRichDocument(raw: string): {
  pages: RichDocPage[];
  pageSize: PageSize;
  pageOrientation: PageOrientation;
} {
  const trimmed = raw.trim();
  if (trimmed.length === 0) {
    return {
      pages: [makeEmptyPage()],
      pageSize: FIXED_PAGE_SIZE,
      pageOrientation: 'portrait',
    };
  }

  try {
    const parsed = JSON.parse(trimmed) as Partial<RichDocument>;
    if (parsed.type === 'rich_doc' && Array.isArray(parsed.pages)) {
      const rawPageOrientation =
        typeof parsed.page_orientation === 'string' ? parsed.page_orientation : '';
      const pageOrientation: PageOrientation = isPageOrientation(rawPageOrientation)
        ? rawPageOrientation
        : 'portrait';
      const pages = normalizePages(
        parsed.pages.map((page) => ({
          id: page.id || createPageId(),
          html: page.html || '',
        })),
      );
      return { pages, pageSize: FIXED_PAGE_SIZE, pageOrientation };
    }
  } catch {
    // Fallback to legacy plain-text content.
  }

  return {
    pages: [
      {
        id: createPageId(),
        html: plainTextToPageHtml(raw),
      },
    ],
    pageSize: FIXED_PAGE_SIZE,
    pageOrientation: 'portrait',
  };
}

function pagesToPlainText(pages: RichDocPage[]): string {
  return pages
    .map((page, index) => {
      const body = htmlToPlainText(page.html).trim();
      if (index === 0) return body;
      return `\n\n--- Page ${index + 1} ---\n\n${body}`;
    })
    .join('')
    .trim();
}

function escapePdfText(value: string): string {
  return value
    .replace(/\\/g, '\\\\')
    .replace(/\(/g, '\\(')
    .replace(/\)/g, '\\)');
}

function buildSimplePdfBytes(text: string, title: string): Uint8Array {
  const linesPerPage = 44;
  const lineWidth = 92;
  const rawLines = text.replace(/\r\n/g, '\n').split('\n');
  const wrapped: string[] = [];

  for (const raw of rawLines) {
    if (raw.length === 0) {
      wrapped.push('');
      continue;
    }
    let start = 0;
    while (start < raw.length) {
      wrapped.push(raw.slice(start, start + lineWidth));
      start += lineWidth;
    }
  }

  const pages: string[][] = [];
  for (let i = 0; i < wrapped.length || i === 0; i += linesPerPage) {
    pages.push(wrapped.slice(i, i + linesPerPage));
  }

  const objects: string[] = [];
  objects.push('<< /Type /Catalog /Pages 2 0 R >>');

  const pageObjectIds: number[] = [];
  const firstPageObjectId = 3;
  for (let i = 0; i < pages.length; i += 1) {
    pageObjectIds.push(firstPageObjectId + i * 2);
  }

  objects.push(
    `<< /Type /Pages /Kids [${pageObjectIds.map((id) => `${id} 0 R`).join(' ')}] /Count ${pages.length} >>`,
  );

  for (let i = 0; i < pages.length; i += 1) {
    const pageId = pageObjectIds[i];
    const contentId = pageId + 1;

    const lines = pages[i];
    const contentLines = [
      'BT',
      '/F1 12 Tf',
      '50 760 Td',
      `(${escapePdfText(title)}) Tj`,
      '0 -24 Td',
    ];

    for (const line of lines) {
      contentLines.push(`(${escapePdfText(line)}) Tj`);
      contentLines.push('0 -16 Td');
    }
    contentLines.push('ET');

    const contentStream = contentLines.join('\n');
    objects.push(
      `<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 ${pageObjectIds[pages.length - 1] + 2} 0 R >> >> /Contents ${contentId} 0 R >>`,
    );
    objects.push(`<< /Length ${contentStream.length} >>\nstream\n${contentStream}\nendstream`);
  }

  objects.push('<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>');

  let pdf = '%PDF-1.4\n';
  const offsets: number[] = [0];
  for (let i = 0; i < objects.length; i += 1) {
    offsets.push(pdf.length);
    pdf += `${i + 1} 0 obj\n${objects[i]}\nendobj\n`;
  }

  const xrefStart = pdf.length;
  pdf += `xref\n0 ${objects.length + 1}\n`;
  pdf += '0000000000 65535 f \n';
  for (let i = 1; i < offsets.length; i += 1) {
    pdf += `${String(offsets[i]).padStart(10, '0')} 00000 n \n`;
  }

  pdf += 'trailer\n';
  pdf += `<< /Size ${objects.length + 1} /Root 1 0 R >>\n`;
  pdf += `startxref\n${xrefStart}\n%%EOF`;

  return new TextEncoder().encode(pdf);
}

function decodePdfString(value: string): string {
  let out = value
    .replace(/\\\(/g, '(')
    .replace(/\\\)/g, ')')
    .replace(/\\\\/g, '\\')
    .replace(/\\r/g, '\r')
    .replace(/\\n/g, '\n')
    .replace(/\\t/g, '\t');

  out = out.replace(/\\([0-7]{1,3})/g, (_m, octal) => {
    const code = Number.parseInt(octal, 8);
    return Number.isNaN(code) ? '' : String.fromCharCode(code);
  });

  return out;
}

function extractPdfText(buffer: ArrayBuffer): string {
  const content = new TextDecoder('latin1').decode(buffer);
  const chunks: string[] = [];

  const tjPattern = /\(([^()]*(?:\\.[^()]*)*)\)\s*Tj/g;
  let tjMatch: RegExpExecArray | null;
  while ((tjMatch = tjPattern.exec(content)) !== null) {
    chunks.push(decodePdfString(tjMatch[1]));
  }

  const tjArrayPattern = /\[([\s\S]*?)\]\s*TJ/g;
  let arrayMatch: RegExpExecArray | null;
  while ((arrayMatch = tjArrayPattern.exec(content)) !== null) {
    const inner = arrayMatch[1];
    const innerPattern = /\(([^()]*(?:\\.[^()]*)*)\)/g;
    let innerMatch: RegExpExecArray | null;
    while ((innerMatch = innerPattern.exec(inner)) !== null) {
      chunks.push(decodePdfString(innerMatch[1]));
    }
  }

  const text = chunks.join('\n').replace(/\n{3,}/g, '\n\n').trim();
  if (!text) {
    throw new Error('Unable to extract text from this PDF. Try a text-based PDF.');
  }
  return text;
}

function commandButtonClass(active = false): string {
  return `inline-flex h-8 w-8 items-center justify-center rounded-md ${
    active
      ? 'btn-primary ring-2 ring-[var(--orange-soft)] ring-offset-1 ring-offset-[rgba(10,14,24,0.95)]'
      : 'btn-secondary'
  }`;
}

function clamp(value: number, min: number, max: number): number {
  if (value < min) return min;
  if (value > max) return max;
  return value;
}

function estimateCanvasBytes(strokes: WsCreateCanvasStroke[]): number {
  try {
    return new TextEncoder().encode(JSON.stringify(strokes)).length;
  } catch {
    return Number.MAX_SAFE_INTEGER;
  }
}

export default function CreateTogetherEditor({
  createState,
  canEdit,
  sendWs,
  activeToolOverride,
}: Props) {
  const [activeTool, setActiveTool] = useState<ActiveTool>('text');
  const [canvasPointerMode, setCanvasPointerMode] = useState<'draw' | 'pan'>('draw');
  const [canvasDragging, setCanvasDragging] = useState(false);
  const [confirmClearCanvasOpen, setConfirmClearCanvasOpen] = useState(false);
  const [documentName, setDocumentName] = useState('Untitled Document');
  const [pages, setPages] = useState<RichDocPage[]>([makeEmptyPage()]);
  const [pageSize, setPageSize] = useState<PageSize>(FIXED_PAGE_SIZE);
  const [pageOrientation, setPageOrientation] = useState<PageOrientation>('portrait');
  const [selectedFontFamily, setSelectedFontFamily] = useState<string>(FONT_OPTIONS[0].css);
  const [selectedFontSizePx, setSelectedFontSizePx] = useState<number>(15);
  const [toolbarState, setToolbarState] = useState<TextToolbarState>(DEFAULT_TOOLBAR_STATE);
  const [activePageId, setActivePageId] = useState<string | null>(null);
  const [canvasStrokes, setCanvasStrokes] = useState<WsCreateCanvasStroke[]>([]);
  const [canvasViewport, setCanvasViewport] = useState<CanvasViewport>({
    scale: 1,
    offsetX: 0,
    offsetY: 0,
  });
  const [brushColor, setBrushColor] = useState('#b95cff');
  const [brushSize, setBrushSize] = useState(4);
  const [localMessage, setLocalMessage] = useState('');

  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const pageRefs = useRef<Record<string, HTMLDivElement | null>>({});
  const pagesRef = useRef<RichDocPage[]>([makeEmptyPage()]);
  const pendingStrokeRef = useRef<WsCreateCanvasStroke | null>(null);
  const activeCanvasPointerIdRef = useRef<number | null>(null);
  const canvasPanDragRef = useRef<{
    startX: number;
    startY: number;
    originOffsetX: number;
    originOffsetY: number;
  } | null>(null);
  const canvasViewportRef = useRef<CanvasViewport>({
    scale: 1,
    offsetX: 0,
    offsetY: 0,
  });
  const pagePatchDebounceRef = useRef<number | null>(null);
  const nameDebounceRef = useRef<number | null>(null);
  const pendingPagePatchRef = useRef<Record<string, string>>({});
  const fileInputRef = useRef<HTMLInputElement | null>(null);
  const latestUpdateRef = useRef<number>(0);

  useEffect(() => {
    pagesRef.current = pages;
  }, [pages]);

  useEffect(() => {
    return () => {
      if (pagePatchDebounceRef.current) window.clearTimeout(pagePatchDebounceRef.current);
      if (nameDebounceRef.current) window.clearTimeout(nameDebounceRef.current);
      activeCanvasPointerIdRef.current = null;
      pendingStrokeRef.current = null;
      canvasPanDragRef.current = null;
      setCanvasDragging(false);
    };
  }, []);

  useEffect(() => {
    if (!canEdit && canvasPointerMode === 'draw') {
      setCanvasPointerMode('pan');
    }
  }, [canEdit, canvasPointerMode]);

  const pushTextState = useCallback(
    (nextText: string, nextFormat: TextFormat = 'plain') => {
      if (!canEdit) return;
      sendWs({
        type: 'create_set_text',
        text_content: nextText,
        text_format: nextFormat,
      });
    },
    [canEdit, sendWs],
  );

  const pushCanvasStroke = useCallback(
    (stroke: WsCreateCanvasStroke) => {
      if (!canEdit) return;
      sendWs({
        type: 'create_canvas_append_stroke',
        canvas_stroke: stroke,
      });
    },
    [canEdit, sendWs],
  );

  const removeCanvasStroke = useCallback(
    (strokeId: string) => {
      if (!canEdit) return;
      sendWs({
        type: 'create_canvas_remove_stroke',
        stroke_id: strokeId,
      });
    },
    [canEdit, sendWs],
  );

  const clearCanvasState = useCallback(() => {
    if (!canEdit) return;
    sendWs({ type: 'create_canvas_clear' });
  }, [canEdit, sendWs]);

  const pushDocumentName = useCallback(
    (nextName: string) => {
      if (!canEdit) return;
      sendWs({
        type: 'create_set_document_name',
        document_name: sanitizeDocumentName(nextName),
      });
    },
    [canEdit, sendWs],
  );

  const flushPagePatches = useCallback(() => {
    if (!canEdit) return;
    const entries = Object.entries(pendingPagePatchRef.current);
    if (entries.length === 0) return;
    pendingPagePatchRef.current = {};
    for (const [pageId, pageHtml] of entries) {
      sendWs({
        type: 'create_upsert_text_page',
        page_id: pageId,
        page_html: pageHtml,
      });
    }
  }, [canEdit, sendWs]);

  const clearQueuedPagePatches = useCallback(() => {
    pendingPagePatchRef.current = {};
    if (pagePatchDebounceRef.current) {
      window.clearTimeout(pagePatchDebounceRef.current);
      pagePatchDebounceRef.current = null;
    }
  }, []);

  const schedulePagePatch = useCallback(
    (pageId: string, pageHtml: string, immediate = false) => {
      if (!canEdit) return;
      pendingPagePatchRef.current[pageId] = pageHtml;

      if (pagePatchDebounceRef.current) {
        window.clearTimeout(pagePatchDebounceRef.current);
        pagePatchDebounceRef.current = null;
      }

      if (immediate) {
        flushPagePatches();
        return;
      }

      pagePatchDebounceRef.current = window.setTimeout(() => {
        flushPagePatches();
        pagePatchDebounceRef.current = null;
      }, 220);
    },
    [canEdit, flushPagePatches],
  );

  useEffect(() => {
    if (!createState) return;
    if (createState.updated_ts_ms < latestUpdateRef.current) return;
    latestUpdateRef.current = createState.updated_ts_ms;

    setActiveTool(createState.active_tool === 'canvas' ? 'canvas' : 'text');
    setDocumentName(sanitizeDocumentName(createState.document_name || 'Untitled Document'));

    const decoded = decodeRichDocument(createState.text_content || '');
    setPageSize(FIXED_PAGE_SIZE);
    setPageOrientation(decoded.pageOrientation);
    pagesRef.current = decoded.pages;
    setPages(decoded.pages);
    setActivePageId((prev) => {
      if (prev && decoded.pages.some((page) => page.id === prev)) return prev;
      return decoded.pages[0]?.id ?? null;
    });

    const incomingStrokes = createState.canvas_strokes || [];
    if (estimateCanvasBytes(incomingStrokes) > CANVAS_MAX_BYTES) {
      setLocalMessage(
        'Incoming canvas data exceeded the 30MB canvas limit and was ignored locally.',
      );
      return;
    }

    setCanvasStrokes(incomingStrokes);
  }, [createState]);

  useEffect(() => {
    if (!activeToolOverride) return;
    setActiveTool(activeToolOverride);
  }, [activeToolOverride]);

  useEffect(() => {
    const known = new Set(pages.map((page) => page.id));
    for (const pageId of Object.keys(pageRefs.current)) {
      if (!known.has(pageId)) {
        delete pageRefs.current[pageId];
      }
    }
  }, [pages]);

  useEffect(() => {
    for (const page of pages) {
      const node = pageRefs.current[page.id];
      if (!node) continue;
      const sanitized = sanitizePageHtml(page.html);
      const current = sanitizePageHtml(node.innerHTML);
      if (current !== sanitized) {
        node.innerHTML = sanitized;
      }
    }
  }, [pages]);

  const canvasMessage = useMemo(() => {
    const zoomPercent = Math.round(canvasViewport.scale * 100);
    if (!canEdit) {
      return `Read-only mode. Pan mode is active. A room admin must enable non-host editing to draw. Zoom ${zoomPercent}% · wheel to pan · pinch/ctrl+wheel to zoom.`;
    }
    return `${canvasPointerMode === 'pan' ? 'Pan mode' : 'Draw mode'} · changes sync for all joined members. Zoom ${zoomPercent}% · wheel to pan · pinch/ctrl+wheel to zoom · 30MB canvas limit.`;
  }, [canEdit, canvasPointerMode, canvasViewport.scale]);

  const clampCanvasViewport = useCallback((next: CanvasViewport): CanvasViewport => {
    const scale = clamp(next.scale, CANVAS_MIN_ZOOM, CANVAS_MAX_ZOOM);

    return {
      scale,
      // Keep panning effectively unlimited while preventing runaway numeric drift.
      offsetX: clamp(next.offsetX, -CANVAS_MAX_PAN_OFFSET, CANVAS_MAX_PAN_OFFSET),
      offsetY: clamp(next.offsetY, -CANVAS_MAX_PAN_OFFSET, CANVAS_MAX_PAN_OFFSET),
    };
  }, []);

  const applyCanvasViewport = useCallback(
    (next: CanvasViewport) => {
      const clampedViewport = clampCanvasViewport(next);
      const current = canvasViewportRef.current;
      if (
        Math.abs(current.scale - clampedViewport.scale) < 1e-6 &&
        Math.abs(current.offsetX - clampedViewport.offsetX) < 1e-3 &&
        Math.abs(current.offsetY - clampedViewport.offsetY) < 1e-3
      ) {
        return;
      }
      canvasViewportRef.current = clampedViewport;
      setCanvasViewport(clampedViewport);
    },
    [clampCanvasViewport],
  );

  const drawStrokes = useCallback(
    (
      strokes: WsCreateCanvasStroke[],
      pending?: WsCreateCanvasStroke | null,
      viewport: CanvasViewport = canvasViewportRef.current,
    ) => {
      const canvas = canvasRef.current;
      if (!canvas) return;

      const ctx = canvas.getContext('2d');
      if (!ctx) return;

      ctx.setTransform(1, 0, 0, 1, 0, 0);
      ctx.clearRect(0, 0, canvas.width, canvas.height);
      // Single surface color so there is no visible moving border as content grows.
      ctx.fillStyle = '#0a0f1f';
      ctx.fillRect(0, 0, canvas.width, canvas.height);

      ctx.setTransform(
        viewport.scale,
        0,
        0,
        viewport.scale,
        viewport.offsetX,
        viewport.offsetY,
      );

      const drawStroke = (stroke: WsCreateCanvasStroke) => {
        if (!stroke.points || stroke.points.length === 0) return;
        ctx.strokeStyle = stroke.color;
        ctx.lineWidth = stroke.size;
        ctx.lineCap = 'round';
        ctx.lineJoin = 'round';
        ctx.beginPath();
        ctx.moveTo(stroke.points[0].x, stroke.points[0].y);
        for (let i = 1; i < stroke.points.length; i += 1) {
          ctx.lineTo(stroke.points[i].x, stroke.points[i].y);
        }
        ctx.stroke();
      };

      for (const stroke of strokes) drawStroke(stroke);
      if (pending) drawStroke(pending);

      ctx.setTransform(1, 0, 0, 1, 0, 0);
    },
    [],
  );

  useEffect(() => {
    drawStrokes(canvasStrokes, pendingStrokeRef.current, canvasViewport);
  }, [canvasStrokes, drawStrokes, canvasViewport]);

  useEffect(() => {
    applyCanvasViewport(canvasViewportRef.current);
  }, [canvasStrokes, applyCanvasViewport]);

  const syncPageFromDom = useCallback(
    (pageId: string, immediate = false) => {
      const node = pageRefs.current[pageId];
      if (!node) return;
      const sanitized = sanitizePageHtml(node.innerHTML);
      const base = pagesRef.current.length > 0 ? pagesRef.current : pages;
      const next = base.map((page) =>
        page.id === pageId
          ? {
              ...page,
              html: sanitized,
            }
          : page,
      );
      pagesRef.current = next;
      setPages(next);
      schedulePagePatch(pageId, sanitized, immediate);
    },
    [pages, schedulePagePatch],
  );

  const snapshotPagesFromDom = useCallback((): RichDocPage[] => {
    const base = pagesRef.current.length > 0 ? pagesRef.current : pages;
    const next = base.map((page) => {
      const node = pageRefs.current[page.id];
      if (!node) return page;
      return {
        ...page,
        html: sanitizePageHtml(node.innerHTML),
      };
    });
    pagesRef.current = next;
    return next;
  }, [pages]);

  const getActiveEditableTarget = useCallback((): { pageId: string; node: HTMLDivElement } | null => {
    const fallbackPageId = pagesRef.current[0]?.id ?? pages[0]?.id ?? null;
    const targetPageId = activePageId ?? fallbackPageId;
    if (!targetPageId) return null;
    const node = pageRefs.current[targetPageId];
    if (!node) return null;
    return { pageId: targetPageId, node };
  }, [activePageId, pages]);

  const updateToolbarState = useCallback(() => {
    if (typeof window === 'undefined' || typeof document === 'undefined') return;
    const target = getActiveEditableTarget();
    if (!target) {
      setToolbarState(DEFAULT_TOOLBAR_STATE);
      return;
    }

    const { node } = target;
    const selection = window.getSelection();
    if (!selection || selection.rangeCount === 0) {
      setToolbarState(DEFAULT_TOOLBAR_STATE);
      return;
    }

    const range = selection.getRangeAt(0);
    if (!node.contains(range.startContainer) || !node.contains(range.endContainer)) {
      setToolbarState(DEFAULT_TOOLBAR_STATE);
      return;
    }

    const queryState = (command: string): boolean => {
      try {
        return document.queryCommandState(command);
      } catch {
        return false;
      }
    };

    const commandAlign = (() => {
      if (queryState('justifyCenter')) return 'center' as const;
      if (queryState('justifyRight')) return 'right' as const;
      if (queryState('justifyFull')) return 'justify' as const;
      if (queryState('justifyLeft')) return 'left' as const;
      return null;
    })();

    let align: TextToolbarState['align'] = commandAlign ?? 'left';
    let cursor: Node | null = selection.anchorNode;
    while (!commandAlign && cursor && cursor !== node) {
      if (cursor.nodeType === Node.ELEMENT_NODE) {
        const el = cursor as HTMLElement;
        const inlineAlign = el.style.textAlign.toLowerCase();
        if (
          inlineAlign === 'left' ||
          inlineAlign === 'center' ||
          inlineAlign === 'right' ||
          inlineAlign === 'justify'
        ) {
          align = inlineAlign;
          break;
        }
        const computedAlign = window.getComputedStyle(el).textAlign.toLowerCase();
        if (
          computedAlign === 'left' ||
          computedAlign === 'center' ||
          computedAlign === 'right' ||
          computedAlign === 'justify'
        ) {
          align = computedAlign;
          break;
        }
      }
      cursor = cursor.parentNode;
    }

    setToolbarState({
      bold: queryState('bold'),
      italic: queryState('italic'),
      align,
      unorderedList: queryState('insertUnorderedList'),
      orderedList: queryState('insertOrderedList'),
    });
  }, [getActiveEditableTarget]);

  useEffect(() => {
    if (typeof document === 'undefined') return;
    const handler = () => updateToolbarState();
    document.addEventListener('selectionchange', handler);
    return () => document.removeEventListener('selectionchange', handler);
  }, [updateToolbarState]);

  useEffect(() => {
    updateToolbarState();
  }, [activePageId, pages, updateToolbarState]);

  const applyCommandToSelection = useCallback(
    (command: 'bold' | 'italic' | 'justifyLeft' | 'justifyCenter' | 'justifyRight' | 'insertUnorderedList' | 'insertOrderedList') => {
      if (!canEdit) return;
      const target = getActiveEditableTarget();
      if (!target) return;
      const { pageId, node } = target;

      node.focus();
      document.execCommand(command, false);
      syncPageFromDom(pageId);
      window.setTimeout(() => updateToolbarState(), 0);
    },
    [canEdit, getActiveEditableTarget, syncPageFromDom, updateToolbarState],
  );

  const applyInlineStyleToSelection = useCallback(
    (stylePatch: { fontFamily?: string; fontSizePx?: number }) => {
      if (!canEdit) return;
      const target = getActiveEditableTarget();
      if (!target) return;
      const { pageId, node } = target;
      node.focus();

      const selection = window.getSelection();
      if (!selection || selection.rangeCount === 0) return;
      const range = selection.getRangeAt(0);
      if (!node.contains(range.startContainer) || !node.contains(range.endContainer)) return;

      if (range.collapsed) {
        let targetEl: HTMLElement | null = null;
        let cursorNode: Node | null = selection.anchorNode;
        while (cursorNode && cursorNode !== node) {
          if (cursorNode.nodeType === Node.ELEMENT_NODE) {
            targetEl = cursorNode as HTMLElement;
            break;
          }
          cursorNode = cursorNode.parentNode;
        }

        if (!targetEl || targetEl === node) {
          targetEl = document.createElement('span');
          range.insertNode(targetEl);
          const nextRange = document.createRange();
          nextRange.selectNodeContents(targetEl);
          nextRange.collapse(false);
          selection.removeAllRanges();
          selection.addRange(nextRange);
        }

        if (stylePatch.fontFamily) {
          targetEl.style.fontFamily = stylePatch.fontFamily;
        }
        if (stylePatch.fontSizePx) {
          targetEl.style.fontSize = `${Math.round(
            clamp(stylePatch.fontSizePx, MIN_FONT_SIZE_PX, MAX_FONT_SIZE_PX),
          )}px`;
        }
      } else {
        const span = document.createElement('span');
        if (stylePatch.fontFamily) {
          span.style.fontFamily = stylePatch.fontFamily;
        }
        if (stylePatch.fontSizePx) {
          span.style.fontSize = `${Math.round(
            clamp(stylePatch.fontSizePx, MIN_FONT_SIZE_PX, MAX_FONT_SIZE_PX),
          )}px`;
        }
        const extracted = range.extractContents();
        span.appendChild(extracted);
        range.insertNode(span);
        const nextRange = document.createRange();
        nextRange.selectNodeContents(span);
        nextRange.collapse(false);
        selection.removeAllRanges();
        selection.addRange(nextRange);
      }

      syncPageFromDom(pageId);
      window.setTimeout(() => updateToolbarState(), 0);
    },
    [canEdit, getActiveEditableTarget, syncPageFromDom, updateToolbarState],
  );

  const handleFontFamilyChange = (nextCss: string) => {
    const safeCss = coerceFontFamily(nextCss);
    if (!safeCss) return;
    setSelectedFontFamily(safeCss);
    applyInlineStyleToSelection({ fontFamily: safeCss });
  };

  const handleFontSizeChange = (nextSizeRaw: string) => {
    const parsed = Number(nextSizeRaw);
    if (!Number.isFinite(parsed)) return;
    const sizePx = Math.round(clamp(parsed, MIN_FONT_SIZE_PX, MAX_FONT_SIZE_PX));
    setSelectedFontSizePx(sizePx);
    applyInlineStyleToSelection({ fontSizePx: sizePx });
  };

  const onNameChange = (next: string) => {
    setDocumentName(next.slice(0, MAX_DOC_NAME));
    if (!canEdit) return;
    if (nameDebounceRef.current) window.clearTimeout(nameDebounceRef.current);
    nameDebounceRef.current = window.setTimeout(() => {
      pushDocumentName(next);
      nameDebounceRef.current = null;
    }, 280);
  };

  const handleAddPage = () => {
    flushPagePatches();
    const page = makeEmptyPage();
    const base = pagesRef.current.length > 0 ? pagesRef.current : pages;
    const next = normalizePages([...base, page]);
    pagesRef.current = next;
    setPages(next);
    setPageSize(FIXED_PAGE_SIZE);
    setActivePageId(page.id);
    if (canEdit) {
      const previousPageId = base.length > 0 ? base[base.length - 1].id : null;
      sendWs({
        type: 'create_insert_text_page',
        page_id: page.id,
        page_html: page.html,
        after_page_id: previousPageId,
      });
    }
    window.setTimeout(() => {
      pageRefs.current[page.id]?.focus();
    }, 0);
  };

  const handleRemovePage = (pageId: string) => {
    if (!canEdit) return;
    if (pages.length <= 1) return;

    flushPagePatches();
    const next = pages.filter((page) => page.id !== pageId);
    pagesRef.current = next;
    setPages(next);
    if (activePageId === pageId) {
      setActivePageId(next[0]?.id ?? null);
    }
    sendWs({
      type: 'create_delete_text_page',
      page_id: pageId,
    });
  };

  const handlePageOrientationChange = (nextPageOrientation: PageOrientation) => {
    setPageOrientation(nextPageOrientation);
    if (!canEdit) return;
    sendWs({
      type: 'create_set_text_page_orientation',
      page_orientation: nextPageOrientation,
    });
  };

  const getCanvasPixelPoint = useCallback((clientX: number, clientY: number): Point => {
    const canvas = canvasRef.current;
    if (!canvas) return { x: 0, y: 0 };
    const rect = canvas.getBoundingClientRect();
    const scaleX = canvas.width / rect.width;
    const scaleY = canvas.height / rect.height;
    return {
      x: (clientX - rect.left) * scaleX,
      y: (clientY - rect.top) * scaleY,
    };
  }, []);

  const getCanvasPoint = (event: ReactPointerEvent<HTMLCanvasElement>): Point => {
    const pixel = getCanvasPixelPoint(event.clientX, event.clientY);
    const viewport = canvasViewportRef.current;
    return {
      x: (pixel.x - viewport.offsetX) / viewport.scale,
      y: (pixel.y - viewport.offsetY) / viewport.scale,
    };
  };

  const onCanvasPointerDown = (event: ReactPointerEvent<HTMLCanvasElement>) => {
    if (event.button !== 0) return;
    activeCanvasPointerIdRef.current = event.pointerId;
    event.currentTarget.setPointerCapture(event.pointerId);

    if (canvasPointerMode === 'pan' || !canEdit) {
      const pixel = getCanvasPixelPoint(event.clientX, event.clientY);
      const currentViewport = canvasViewportRef.current;
      canvasPanDragRef.current = {
        startX: pixel.x,
        startY: pixel.y,
        originOffsetX: currentViewport.offsetX,
        originOffsetY: currentViewport.offsetY,
      };
      pendingStrokeRef.current = null;
      setCanvasDragging(true);
      return;
    }

    if (!canEdit) return;
    const point = getCanvasPoint(event);
    const stroke: WsCreateCanvasStroke = {
      id: `${Date.now()}-${Math.random().toString(36).slice(2, 10)}`,
      color: brushColor,
      size: brushSize,
      points: [point],
    };
    pendingStrokeRef.current = stroke;
    setCanvasDragging(false);
    drawStrokes(canvasStrokes, stroke);
  };

  const onCanvasPointerMove = (event: ReactPointerEvent<HTMLCanvasElement>) => {
    if (activeCanvasPointerIdRef.current !== event.pointerId) return;
    const panDrag = canvasPanDragRef.current;
    if (panDrag) {
      const pixel = getCanvasPixelPoint(event.clientX, event.clientY);
      applyCanvasViewport({
        ...canvasViewportRef.current,
        offsetX: panDrag.originOffsetX + (pixel.x - panDrag.startX),
        offsetY: panDrag.originOffsetY + (pixel.y - panDrag.startY),
      });
      return;
    }
    const pending = pendingStrokeRef.current;
    if (!pending || !canEdit) return;
    const point = getCanvasPoint(event);
    pending.points.push(point);
    drawStrokes(canvasStrokes, pending);
  };

  const finishStroke = (event: ReactPointerEvent<HTMLCanvasElement>) => {
    if (activeCanvasPointerIdRef.current !== event.pointerId) return;
    try {
      event.currentTarget.releasePointerCapture(event.pointerId);
    } catch {
      // Pointer may already be released.
    }
    activeCanvasPointerIdRef.current = null;
    setCanvasDragging(false);
    if (canvasPanDragRef.current) {
      canvasPanDragRef.current = null;
      pendingStrokeRef.current = null;
      return;
    }

    const pending = pendingStrokeRef.current;
    if (!pending || !canEdit) {
      pendingStrokeRef.current = null;
      return;
    }
    pendingStrokeRef.current = null;

    const finalized =
      pending.points.length > 1
        ? pending
        : { ...pending, points: [...pending.points, pending.points[0]] };
    const next = [...canvasStrokes, finalized];
    if (estimateCanvasBytes(next) > CANVAS_MAX_BYTES) {
      setLocalMessage('Canvas storage limit reached (30MB). Undo or clear strokes to continue.');
      drawStrokes(canvasStrokes, null, canvasViewportRef.current);
      return;
    }
    setCanvasStrokes(next);
    setLocalMessage('');
    pushCanvasStroke(finalized);
  };

  const handleCanvasWheel = useCallback((event: CanvasWheelEventLike) => {
    event.preventDefault();
    event.stopPropagation?.();
    const prev = canvasViewportRef.current;
    const anchor = getCanvasPixelPoint(event.clientX, event.clientY);

    if (event.ctrlKey || event.metaKey) {
      const zoomFactor = Math.exp(-event.deltaY * CANVAS_WHEEL_ZOOM_SENSITIVITY);
      const nextScale = clamp(prev.scale * zoomFactor, CANVAS_MIN_ZOOM, CANVAS_MAX_ZOOM);
      const worldX = (anchor.x - prev.offsetX) / prev.scale;
      const worldY = (anchor.y - prev.offsetY) / prev.scale;
      applyCanvasViewport({
        scale: nextScale,
        offsetX: anchor.x - worldX * nextScale,
        offsetY: anchor.y - worldY * nextScale,
      });
      return;
    }

    applyCanvasViewport({
      ...prev,
      offsetX: prev.offsetX - event.deltaX,
      offsetY: prev.offsetY - event.deltaY,
    });
  }, [applyCanvasViewport, getCanvasPixelPoint]);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const nativeWheelListener = (event: WheelEvent) => {
      handleCanvasWheel(event);
    };
    canvas.addEventListener('wheel', nativeWheelListener, { passive: false });
    return () => {
      canvas.removeEventListener('wheel', nativeWheelListener);
    };
  }, [handleCanvasWheel]);

  const setCanvasZoomAroundPoint = useCallback((nextScaleRaw: number, anchor: Point) => {
    const prev = canvasViewportRef.current;
    const nextScale = clamp(nextScaleRaw, CANVAS_MIN_ZOOM, CANVAS_MAX_ZOOM);
    const worldX = (anchor.x - prev.offsetX) / prev.scale;
    const worldY = (anchor.y - prev.offsetY) / prev.scale;
    applyCanvasViewport({
      scale: nextScale,
      offsetX: anchor.x - worldX * nextScale,
      offsetY: anchor.y - worldY * nextScale,
    });
  }, [applyCanvasViewport]);

  const adjustCanvasZoom = (multiplier: number) => {
    const canvas = canvasRef.current;
    const center = {
      x: (canvas?.width ?? CANVAS_WIDTH) / 2,
      y: (canvas?.height ?? CANVAS_HEIGHT) / 2,
    };
    setCanvasZoomAroundPoint(canvasViewportRef.current.scale * multiplier, center);
  };

  const handleCanvasZoomSliderChange = (event: ChangeEvent<HTMLInputElement>) => {
    const canvas = canvasRef.current;
    const center = {
      x: (canvas?.width ?? CANVAS_WIDTH) / 2,
      y: (canvas?.height ?? CANVAS_HEIGHT) / 2,
    };
    setCanvasZoomAroundPoint(Number(event.target.value) / 100, center);
  };

  const resetCanvasViewport = () => {
    applyCanvasViewport({ scale: 1, offsetX: 0, offsetY: 0 });
  };

  const handleCanvasUndo = () => {
    if (!canEdit || canvasStrokes.length === 0) return;
    const removed = canvasStrokes[canvasStrokes.length - 1];
    const next = canvasStrokes.slice(0, -1);
    setCanvasStrokes(next);
    removeCanvasStroke(removed.id);
  };

  const handleCanvasClear = () => {
    if (!canEdit) return;
    setCanvasStrokes([]);
    clearCanvasState();
    setConfirmClearCanvasOpen(false);
  };

  const handleDownloadText = (format: 'txt' | 'md') => {
    const ext = format === 'md' ? 'md' : 'txt';
    const mime =
      format === 'md' ? 'text/markdown;charset=utf-8' : 'text/plain;charset=utf-8';
    const latestPages = snapshotPagesFromDom();
    setPages(latestPages);
    const plain = pagesToPlainText(latestPages);
    downloadBlob(new Blob([plain], { type: mime }), `${sanitizeDocumentName(documentName)}.${ext}`);
  };

  const handleDownloadPdf = () => {
    const latestPages = snapshotPagesFromDom();
    setPages(latestPages);
    const plain = pagesToPlainText(latestPages);
    const bytes = buildSimplePdfBytes(plain, sanitizeDocumentName(documentName));
    const pdfBuffer = bytes.buffer.slice(
      bytes.byteOffset,
      bytes.byteOffset + bytes.byteLength,
    ) as ArrayBuffer;
    downloadBlob(
      new Blob([pdfBuffer], { type: 'application/pdf' }),
      `${sanitizeDocumentName(documentName)}.pdf`,
    );
  };

  const handleDownloadCanvasPng = () => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    canvas.toBlob((blob) => {
      if (!blob) return;
      downloadBlob(blob, `${sanitizeDocumentName(documentName)}.png`);
    }, 'image/png');
  };

  const handleImportFile = async (event: ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    if (!file) return;

    try {
      const name = fileBaseName(file.name);
      setDocumentName(name);
      pushDocumentName(name);
      setActiveTool('text');
      if (canEdit) {
        sendWs({ type: 'create_set_tool', tool: 'text' });
      }

      const lower = file.name.toLowerCase();
      let importedText = '';

      if (lower.endsWith('.pdf')) {
        const buffer = await file.arrayBuffer();
        importedText = extractPdfText(buffer);
      } else {
        importedText = await file.text();
      }

      const page: RichDocPage = {
        id: createPageId(),
        html: plainTextToPageHtml(importedText),
      };
      const importedPages = [page];
      clearQueuedPagePatches();
      pagesRef.current = importedPages;
      setPages(importedPages);
      setPageSize(FIXED_PAGE_SIZE);
      setPageOrientation(pageOrientation);
      if (canEdit) {
        const payload = serializeRichDocument(importedPages, FIXED_PAGE_SIZE, pageOrientation);
        pushTextState(payload, 'plain');
      }
      setActivePageId(page.id);
      setLocalMessage(`Loaded ${file.name}`);
    } catch (err: unknown) {
      setLocalMessage(clientErrorMessage(err, 'Failed to import file'));
    } finally {
      event.target.value = '';
    }
  };

  const baseMetrics = PAGE_SIZES[pageSize];
  const metrics =
    pageOrientation === 'landscape'
      ? {
          label: `${baseMetrics.label} Landscape`,
          widthPx: baseMetrics.heightPx,
          heightPx: baseMetrics.widthPx,
        }
      : {
          label: `${baseMetrics.label} Portrait`,
          widthPx: baseMetrics.widthPx,
          heightPx: baseMetrics.heightPx,
        };

  return (
    <section className="space-y-4">
      <div className="space-y-2">
        <h2 className="text-xl font-semibold">Create Together</h2>
        <p className="text-sm muted">
          Collaborative paged documents + shared canvas. Use rich text controls and add pages like a standard document editor.
        </p>
      </div>

      <label className="block text-sm">
        <span className="mb-1 block text-xs uppercase tracking-wide muted">Document Name</span>
        <input
          type="text"
          value={documentName}
          onChange={(event) => onNameChange(event.target.value)}
          className="input px-3 py-2 text-sm"
          maxLength={MAX_DOC_NAME}
          disabled={!canEdit}
        />
      </label>

      {activeTool === 'text' ? (
        <div className="space-y-3">
          <div className="flex flex-wrap items-center gap-2">
            <label className="text-xs uppercase tracking-wide muted">Orientation</label>
            <select
              className="select px-3 py-2 text-sm"
              value={pageOrientation}
              onChange={(event) => {
                const next = event.target.value;
                if (!isPageOrientation(next)) return;
                handlePageOrientationChange(next);
              }}
              disabled={!canEdit}
            >
              <option value="portrait">Portrait</option>
              <option value="landscape">Landscape</option>
            </select>

            <button
              type="button"
              className="btn-secondary px-3 py-2 text-sm"
              onClick={handleAddPage}
              disabled={!canEdit || pages.length >= MAX_DOC_PAGES}
            >
              Add Page
            </button>

            <input
              ref={fileInputRef}
              type="file"
              accept=".txt,.md,.pdf,text/plain,text/markdown,application/pdf"
              className="hidden"
              onChange={handleImportFile}
              disabled={!canEdit}
            />
            <button
              type="button"
              className="btn-secondary px-3 py-2 text-sm"
              onClick={() => fileInputRef.current?.click()}
              disabled={!canEdit}
            >
              Import TXT/MD/PDF
            </button>

            <button
              type="button"
              className="btn-secondary px-3 py-2 text-sm"
              onClick={() => handleDownloadText('txt')}
            >
              Download TXT
            </button>
            <button
              type="button"
              className="btn-secondary px-3 py-2 text-sm"
              onClick={() => handleDownloadText('md')}
            >
              Download MD
            </button>
            <button
              type="button"
              className="btn-secondary px-3 py-2 text-sm"
              onClick={handleDownloadPdf}
            >
              Download PDF
            </button>
          </div>

          <div className="flex flex-wrap items-center gap-2">
            <span className="text-xs uppercase tracking-wide muted">Format</span>
            <select
              className="select h-8 min-w-[9.5rem] px-2 text-xs"
              value={selectedFontFamily}
              onChange={(event) => handleFontFamilyChange(event.target.value)}
              disabled={!canEdit}
              aria-label="Font family"
              title="Font family"
            >
              {FONT_OPTIONS.map((option) => (
                <option key={option.css} value={option.css}>
                  {option.label}
                </option>
              ))}
            </select>
            <select
              className="select h-8 w-[4.5rem] px-2 text-xs"
              value={selectedFontSizePx}
              onChange={(event) => handleFontSizeChange(event.target.value)}
              disabled={!canEdit}
              aria-label="Font size"
              title="Font size"
            >
              {FONT_SIZE_OPTIONS.map((size) => (
                <option key={size} value={size}>
                  {size}
                </option>
              ))}
            </select>
            <button
              type="button"
              className={commandButtonClass(toolbarState.bold)}
              onClick={() => applyCommandToSelection('bold')}
              disabled={!canEdit}
              title="Bold"
              aria-label="Bold"
            >
              <span
                className="text-[15px] font-bold leading-none"
                style={{ fontFamily: 'Georgia, "Times New Roman", serif' }}
              >
                B
              </span>
            </button>
            <button
              type="button"
              className={commandButtonClass(toolbarState.italic)}
              onClick={() => applyCommandToSelection('italic')}
              disabled={!canEdit}
              title="Italic"
              aria-label="Italic"
            >
              <span className="text-sm font-semibold italic">I</span>
            </button>
            <button
              type="button"
              className={commandButtonClass(toolbarState.align === 'left')}
              onClick={() => applyCommandToSelection('justifyLeft')}
              disabled={!canEdit}
              title="Align Left"
              aria-label="Align Left"
            >
              <svg viewBox="0 0 16 16" className="h-4 w-4" fill="none" aria-hidden="true">
                <path d="M2 3h10M2 6h7M2 9h10M2 12h7" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />
              </svg>
            </button>
            <button
              type="button"
              className={commandButtonClass(toolbarState.align === 'center')}
              onClick={() => applyCommandToSelection('justifyCenter')}
              disabled={!canEdit}
              title="Align Center"
              aria-label="Align Center"
            >
              <svg viewBox="0 0 16 16" className="h-4 w-4" fill="none" aria-hidden="true">
                <path d="M3 3h10M5 6h6M3 9h10M5 12h6" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />
              </svg>
            </button>
            <button
              type="button"
              className={commandButtonClass(toolbarState.align === 'right')}
              onClick={() => applyCommandToSelection('justifyRight')}
              disabled={!canEdit}
              title="Align Right"
              aria-label="Align Right"
            >
              <svg viewBox="0 0 16 16" className="h-4 w-4" fill="none" aria-hidden="true">
                <path d="M4 3h10M7 6h7M4 9h10M7 12h7" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />
              </svg>
            </button>
            <button
              type="button"
              className={commandButtonClass(toolbarState.unorderedList)}
              onClick={() => applyCommandToSelection('insertUnorderedList')}
              disabled={!canEdit}
              title="Bulleted List"
              aria-label="Bulleted List"
            >
              <svg viewBox="0 0 16 16" className="h-4 w-4" fill="none" aria-hidden="true">
                <circle cx="3" cy="4" r="1.2" fill="currentColor" />
                <circle cx="3" cy="8" r="1.2" fill="currentColor" />
                <circle cx="3" cy="12" r="1.2" fill="currentColor" />
                <path d="M6 4h7M6 8h7M6 12h7" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />
              </svg>
            </button>
            <button
              type="button"
              className={commandButtonClass(toolbarState.orderedList)}
              onClick={() => applyCommandToSelection('insertOrderedList')}
              disabled={!canEdit}
              title="Numbered List"
              aria-label="Numbered List"
            >
              <svg viewBox="0 0 16 16" className="h-4 w-4" fill="none" aria-hidden="true">
                <path d="M1.6 3.6h2M2.6 2.8v3.1M1.6 7.5h2M1.6 11.2h2M1.6 11.2l2 2H1.6" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round" />
                <path d="M6 4h7M6 8h7M6 12h7" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />
              </svg>
            </button>
          </div>

          <div className="overflow-hidden rounded-xl border border-white/10 bg-black/30 p-3">
            <div className="max-h-[74vh] overflow-auto pr-1">
              <div className="mx-auto flex w-full flex-col items-center gap-6 pb-2">
                {pages.map((page, index) => (
                  <div key={page.id} className="w-full">
                    <div className="mx-auto mb-2 flex w-full items-center justify-between text-xs muted" style={{ maxWidth: `${metrics.widthPx}px` }}>
                      <span>
                        Page {index + 1} · {metrics.label}
                      </span>
                      <button
                        type="button"
                        className="btn-ghost px-2 py-0.5 text-xs"
                        disabled={!canEdit || pages.length <= 1}
                        onClick={() => handleRemovePage(page.id)}
                      >
                        Remove
                      </button>
                    </div>
                    <div
                      className="mx-auto w-full rounded-md border border-black/20 bg-white text-black shadow-[0_18px_40px_rgba(0,0,0,0.38)]"
                      style={{
                        maxWidth: `${metrics.widthPx}px`,
                        minHeight: `${metrics.heightPx}px`,
                      }}
                    >
                      <div
                        ref={(node) => {
                          if (!node) {
                            delete pageRefs.current[page.id];
                            return;
                          }
                          pageRefs.current[page.id] = node;
                          const sanitized = sanitizePageHtml(page.html);
                          const current = sanitizePageHtml(node.innerHTML);
                          if (current !== sanitized) {
                            node.innerHTML = sanitized;
                          }
                        }}
                        contentEditable={canEdit}
                        suppressContentEditableWarning
                        className="outline-none px-14 py-14 text-[15px] leading-7"
                        style={{ minHeight: `${metrics.heightPx - 112}px` }}
                        onFocus={() => {
                          setActivePageId(page.id);
                          window.setTimeout(() => updateToolbarState(), 0);
                        }}
                        onInput={(event) => {
                          const html = (event.currentTarget as HTMLDivElement).innerHTML;
                          const sanitized = sanitizePageHtml(html);
                          const base = pagesRef.current.length > 0 ? pagesRef.current : pages;
                          const next = base.map((entry) =>
                            entry.id === page.id
                              ? {
                                  ...entry,
                                  html: sanitized,
                                }
                              : entry,
                          );
                          pagesRef.current = next;
                          schedulePagePatch(page.id, sanitized);
                          window.setTimeout(() => updateToolbarState(), 0);
                        }}
                        onMouseUp={() => updateToolbarState()}
                        onKeyUp={() => updateToolbarState()}
                        onBlur={() => syncPageFromDom(page.id)}
                      />
                    </div>
                  </div>
                ))}
              </div>
            </div>
          </div>

          <p className="text-xs muted">
            {canEdit
              ? 'Paged edits sync in realtime for everyone in the room.'
              : 'Read-only mode. A room admin must enable non-host editing.'}
          </p>
        </div>
      ) : (
        <div className="space-y-3">
          <div className="flex flex-wrap items-center gap-2">
            <label className="text-xs uppercase tracking-wide muted">Mode</label>
            <div className="inline-flex items-center gap-1 rounded-lg border border-white/10 bg-black/25 p-1">
              <button
                type="button"
                className={`${canvasPointerMode === 'pan' ? 'btn-primary' : 'btn-secondary'} inline-flex h-8 w-8 items-center justify-center p-0`}
                onClick={() => setCanvasPointerMode('pan')}
                title="Pan canvas"
                aria-label="Pan canvas"
              >
                <svg viewBox="0 0 24 24" className="h-4 w-4" fill="none" aria-hidden="true">
                  <rect x="7" y="2.8" width="10" height="18.4" rx="5" stroke="currentColor" strokeWidth="1.8" />
                  <path d="M12 6.8v3.2" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
                </svg>
              </button>
              <button
                type="button"
                className={`${canvasPointerMode === 'draw' ? 'btn-primary' : 'btn-secondary'} inline-flex h-8 w-8 items-center justify-center p-0 disabled:opacity-40`}
                onClick={() => setCanvasPointerMode('draw')}
                disabled={!canEdit}
                title={canEdit ? 'Draw on canvas' : 'Drawing disabled in read-only mode'}
                aria-label="Draw on canvas"
              >
                <svg viewBox="0 0 24 24" className="h-4 w-4" fill="none" aria-hidden="true">
                  <path d="M4 20h4l10.2-10.2-4-4L4 16v4Z" stroke="currentColor" strokeWidth="1.8" strokeLinejoin="round" />
                  <path d="m13.8 5.8 4 4" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
                </svg>
              </button>
            </div>

            <label className="text-xs uppercase tracking-wide muted">Brush Color</label>
            <input
              type="color"
              value={brushColor}
              onChange={(event) => setBrushColor(event.target.value)}
              disabled={!canEdit}
            />
            <label className="text-xs uppercase tracking-wide muted">Brush Size</label>
            <input
              type="range"
              min={1}
              max={24}
              value={brushSize}
              onChange={(event) => setBrushSize(Number(event.target.value))}
              disabled={!canEdit}
            />
            <span className="text-xs muted">{brushSize}px</span>
            <button
              type="button"
              className="btn-secondary px-3 py-2 text-sm"
              onClick={handleCanvasUndo}
              disabled={!canEdit || canvasStrokes.length === 0}
            >
              Undo
            </button>
            <button
              type="button"
              className="btn-secondary px-3 py-2 text-sm"
              onClick={() => setConfirmClearCanvasOpen(true)}
              disabled={!canEdit}
            >
              Clear
            </button>
            <button
              type="button"
              className="btn-secondary px-3 py-2 text-sm"
              onClick={handleDownloadCanvasPng}
            >
              Download PNG
            </button>
            <button
              type="button"
              className="inline-flex h-8 w-8 items-center justify-center rounded-full bg-transparent p-0 text-sm text-white transition hover:bg-white/8"
              onClick={() => adjustCanvasZoom(0.85)}
              aria-label="Zoom out"
              title="Zoom out"
            >
              -
            </button>
            <span className="rounded-md border border-white/10 px-2 py-1 text-xs muted">
              Zoom {Math.round(canvasViewport.scale * 100)}%
            </span>
            <input
              type="range"
              min={1}
              max={200}
              step={1}
              value={Math.round(canvasViewport.scale * 100)}
              onChange={handleCanvasZoomSliderChange}
              className="h-2 w-32 accent-white"
              aria-label="Canvas zoom"
              title="Canvas zoom"
            />
            <span className="w-12 text-right text-xs muted">1-200%</span>
            <button
              type="button"
              className="inline-flex h-8 w-8 items-center justify-center rounded-full bg-transparent p-0 text-sm text-white transition hover:bg-white/8"
              onClick={() => adjustCanvasZoom(1.15)}
              aria-label="Zoom in"
              title="Zoom in"
            >
              +
            </button>
            <button
              type="button"
              className="btn-secondary px-3 py-2 text-sm"
              onClick={resetCanvasViewport}
            >
              Recentre View
            </button>
          </div>

          <div className="overflow-hidden rounded-xl border border-white/10 bg-black/40">
            <canvas
              ref={canvasRef}
              width={CANVAS_WIDTH}
              height={CANVAS_HEIGHT}
              className={`h-auto w-full touch-none ${
                canvasPointerMode === 'pan' || !canEdit
                  ? canvasDragging
                    ? 'cursor-grabbing'
                    : 'cursor-grab'
                  : 'cursor-crosshair'
              }`}
              onPointerDown={onCanvasPointerDown}
              onPointerMove={onCanvasPointerMove}
              onPointerUp={finishStroke}
              onPointerCancel={finishStroke}
              onPointerLeave={finishStroke}
            />
          </div>
          <p className="text-xs muted">{canvasMessage}</p>
        </div>
      )}

      {confirmClearCanvasOpen ? (
        <div className="fixed inset-0 z-[150] flex items-center justify-center bg-black/55 p-4 backdrop-blur-[2px]">
          <div className="panel w-full max-w-md space-y-4 rounded-2xl border border-[var(--border)] p-6">
            <div className="space-y-2">
              <h2 className="text-lg font-semibold">Clear Canvas</h2>
              <p className="text-sm muted">
                Remove every stroke from this shared canvas for everyone in the room? This cannot
                be undone.
              </p>
            </div>

            <div className="flex justify-end gap-2">
              <button
                type="button"
                onClick={() => setConfirmClearCanvasOpen(false)}
                className="btn-ghost px-4 py-2 text-sm"
              >
                Cancel
              </button>
              <button
                type="button"
                onClick={handleCanvasClear}
                className="rounded-xl bg-red-500 px-4 py-2 text-sm font-medium text-white transition hover:bg-red-600"
              >
                Clear canvas
              </button>
            </div>
          </div>
        </div>
      ) : null}

      {localMessage && <div className="notice-ok rounded-xl px-3 py-2 text-xs">{localMessage}</div>}
    </section>
  );
}

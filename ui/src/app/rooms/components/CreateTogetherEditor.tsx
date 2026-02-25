'use client';

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { ChangeEvent, PointerEvent as ReactPointerEvent } from 'react';
import type { WsCreateCanvasStroke, WsCreateStateMessage } from '@/lib/watchPartyApi';

type Props = {
  createState: WsCreateStateMessage | null;
  canEdit: boolean;
  sendWs: (payload: Record<string, unknown>) => boolean;
};

type Point = { x: number; y: number };

type ActiveTool = 'text' | 'canvas';
type TextFormat = 'plain' | 'markdown' | 'pdf_text';
type PageSize = 'a4' | 'letter';

type RichDocPage = {
  id: string;
  html: string;
};

type RichDocument = {
  version: 1;
  type: 'rich_doc';
  page_size: PageSize;
  pages: RichDocPage[];
};

const MAX_DOC_NAME = 120;
const MAX_DOC_PAGES = 80;
const EMPTY_PAGE_HTML = '<p><br></p>';

const PAGE_SIZES: Record<PageSize, { label: string; widthPx: number; heightPx: number }> = {
  a4: { label: 'A4', widthPx: 794, heightPx: 1123 },
  letter: { label: 'Letter', widthPx: 816, heightPx: 1056 },
};

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

function sanitizeStyleValue(styleValue: string): string | null {
  const match = styleValue.match(/text-align\s*:\s*(left|center|right|justify)/i);
  if (!match) return null;
  return `text-align:${match[1].toLowerCase()};`;
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

function serializeRichDocument(pages: RichDocPage[], pageSize: PageSize): string {
  const payload: RichDocument = {
    version: 1,
    type: 'rich_doc',
    page_size: pageSize,
    pages: normalizePages(pages),
  };
  return JSON.stringify(payload);
}

function decodeRichDocument(raw: string): { pages: RichDocPage[]; pageSize: PageSize } {
  const trimmed = raw.trim();
  if (trimmed.length === 0) {
    return {
      pages: [makeEmptyPage()],
      pageSize: 'a4',
    };
  }

  try {
    const parsed = JSON.parse(trimmed) as Partial<RichDocument>;
    if (parsed.type === 'rich_doc' && Array.isArray(parsed.pages)) {
      const pageSize = parsed.page_size === 'letter' ? 'letter' : 'a4';
      const pages = normalizePages(
        parsed.pages.map((page) => ({
          id: page.id || createPageId(),
          html: page.html || '',
        })),
      );
      return { pages, pageSize };
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
    pageSize: 'a4',
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
  return `rounded-md px-3 py-1.5 text-xs ${active ? 'btn-primary' : 'btn-secondary'}`;
}

export default function CreateTogetherEditor({ createState, canEdit, sendWs }: Props) {
  const [activeTool, setActiveTool] = useState<ActiveTool>('text');
  const [documentName, setDocumentName] = useState('Untitled Document');
  const [pages, setPages] = useState<RichDocPage[]>([makeEmptyPage()]);
  const [pageSize, setPageSize] = useState<PageSize>('a4');
  const [activePageId, setActivePageId] = useState<string | null>(null);
  const [canvasStrokes, setCanvasStrokes] = useState<WsCreateCanvasStroke[]>([]);
  const [brushColor, setBrushColor] = useState('#b95cff');
  const [brushSize, setBrushSize] = useState(4);
  const [localMessage, setLocalMessage] = useState('');

  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const pageRefs = useRef<Record<string, HTMLDivElement | null>>({});
  const pendingStrokeRef = useRef<WsCreateCanvasStroke | null>(null);
  const textDebounceRef = useRef<number | null>(null);
  const nameDebounceRef = useRef<number | null>(null);
  const fileInputRef = useRef<HTMLInputElement | null>(null);
  const latestUpdateRef = useRef<number>(0);

  useEffect(() => {
    return () => {
      if (textDebounceRef.current) window.clearTimeout(textDebounceRef.current);
      if (nameDebounceRef.current) window.clearTimeout(nameDebounceRef.current);
    };
  }, []);

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

  const pushCanvasState = useCallback(
    (nextStrokes: WsCreateCanvasStroke[]) => {
      if (!canEdit) return;
      sendWs({
        type: 'create_set_canvas',
        canvas_strokes: nextStrokes,
      });
    },
    [canEdit, sendWs],
  );

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

  const setToolAndBroadcast = useCallback(
    (next: ActiveTool) => {
      setActiveTool(next);
      if (!canEdit) return;
      sendWs({ type: 'create_set_tool', tool: next });
    },
    [canEdit, sendWs],
  );

  const scheduleDocumentSync = useCallback(
    (nextPages: RichDocPage[], nextPageSize: PageSize, immediate = false) => {
      if (!canEdit) return;
      const normalized = normalizePages(nextPages);
      const payload = serializeRichDocument(normalized, nextPageSize);

      if (textDebounceRef.current) {
        window.clearTimeout(textDebounceRef.current);
        textDebounceRef.current = null;
      }

      if (immediate) {
        pushTextState(payload, 'plain');
        return;
      }

      textDebounceRef.current = window.setTimeout(() => {
        pushTextState(payload, 'plain');
        textDebounceRef.current = null;
      }, 220);
    },
    [canEdit, pushTextState],
  );

  const commitDocument = useCallback(
    (nextPages: RichDocPage[], nextPageSize: PageSize, immediate = false) => {
      const normalized = normalizePages(nextPages);
      setPages(normalized);
      setPageSize(nextPageSize);
      scheduleDocumentSync(normalized, nextPageSize, immediate);
    },
    [scheduleDocumentSync],
  );

  useEffect(() => {
    if (!createState) return;
    if (createState.updated_ts_ms < latestUpdateRef.current) return;
    latestUpdateRef.current = createState.updated_ts_ms;

    setActiveTool(createState.active_tool === 'canvas' ? 'canvas' : 'text');
    setDocumentName(sanitizeDocumentName(createState.document_name || 'Untitled Document'));

    const decoded = decodeRichDocument(createState.text_content || '');
    setPageSize(decoded.pageSize);
    setPages(decoded.pages);
    setActivePageId((prev) => {
      if (prev && decoded.pages.some((page) => page.id === prev)) return prev;
      return decoded.pages[0]?.id ?? null;
    });

    setCanvasStrokes(createState.canvas_strokes || []);
  }, [createState]);

  useEffect(() => {
    const known = new Set(pages.map((page) => page.id));
    for (const pageId of Object.keys(pageRefs.current)) {
      if (!known.has(pageId)) {
        delete pageRefs.current[pageId];
      }
    }
  }, [pages]);

  const canvasMessage = useMemo(() => {
    if (!canEdit) return 'Read-only mode. A room admin must enable non-host editing.';
    return 'Draw with mouse or touch. Changes sync for all joined members.';
  }, [canEdit]);

  const drawStrokes = useCallback(
    (strokes: WsCreateCanvasStroke[], pending?: WsCreateCanvasStroke | null) => {
      const canvas = canvasRef.current;
      if (!canvas) return;

      const ctx = canvas.getContext('2d');
      if (!ctx) return;

      ctx.clearRect(0, 0, canvas.width, canvas.height);
      ctx.fillStyle = '#0a0f1f';
      ctx.fillRect(0, 0, canvas.width, canvas.height);

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
    },
    [],
  );

  useEffect(() => {
    drawStrokes(canvasStrokes, pendingStrokeRef.current);
  }, [canvasStrokes, drawStrokes]);

  const syncPageFromDom = useCallback(
    (pageId: string, immediate = false) => {
      const node = pageRefs.current[pageId];
      if (!node) return;
      const sanitized = sanitizePageHtml(node.innerHTML);
      setPages((prev) => {
        const next = prev.map((page) =>
          page.id === pageId
            ? {
                ...page,
                html: sanitized,
              }
            : page,
        );
        scheduleDocumentSync(next, pageSize, immediate);
        return next;
      });
    },
    [pageSize, scheduleDocumentSync],
  );

  const applyCommandToSelection = useCallback(
    (command: 'bold' | 'italic' | 'justifyLeft' | 'justifyCenter' | 'justifyRight' | 'insertUnorderedList' | 'insertOrderedList') => {
      if (!canEdit) return;
      const targetPageId = activePageId ?? pages[0]?.id;
      if (!targetPageId) return;
      const node = pageRefs.current[targetPageId];
      if (!node) return;

      node.focus();
      document.execCommand(command, false);
      syncPageFromDom(targetPageId);
    },
    [canEdit, activePageId, pages, syncPageFromDom],
  );

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
    const page = makeEmptyPage();
    const next = [...pages, page];
    commitDocument(next, pageSize, true);
    setActivePageId(page.id);
    window.setTimeout(() => {
      pageRefs.current[page.id]?.focus();
    }, 0);
  };

  const handleRemovePage = (pageId: string) => {
    if (!canEdit) return;
    if (pages.length <= 1) return;

    const next = pages.filter((page) => page.id !== pageId);
    commitDocument(next, pageSize, true);
    if (activePageId === pageId) {
      setActivePageId(next[0]?.id ?? null);
    }
  };

  const handlePageSizeChange = (nextPageSize: PageSize) => {
    setPageSize(nextPageSize);
    scheduleDocumentSync(pages, nextPageSize, true);
  };

  const getCanvasPoint = (event: ReactPointerEvent<HTMLCanvasElement>): Point => {
    const canvas = canvasRef.current;
    if (!canvas) return { x: 0, y: 0 };
    const rect = canvas.getBoundingClientRect();
    const scaleX = canvas.width / rect.width;
    const scaleY = canvas.height / rect.height;
    return {
      x: (event.clientX - rect.left) * scaleX,
      y: (event.clientY - rect.top) * scaleY,
    };
  };

  const onCanvasPointerDown = (event: ReactPointerEvent<HTMLCanvasElement>) => {
    if (!canEdit) return;
    const point = getCanvasPoint(event);
    const stroke: WsCreateCanvasStroke = {
      id: `${Date.now()}-${Math.random().toString(36).slice(2, 10)}`,
      color: brushColor,
      size: brushSize,
      points: [point],
    };
    pendingStrokeRef.current = stroke;
    event.currentTarget.setPointerCapture(event.pointerId);
    drawStrokes(canvasStrokes, stroke);
  };

  const onCanvasPointerMove = (event: ReactPointerEvent<HTMLCanvasElement>) => {
    const pending = pendingStrokeRef.current;
    if (!pending || !canEdit) return;
    const point = getCanvasPoint(event);
    pending.points.push(point);
    drawStrokes(canvasStrokes, pending);
  };

  const finishStroke = (event: ReactPointerEvent<HTMLCanvasElement>) => {
    const pending = pendingStrokeRef.current;
    if (!pending || !canEdit) return;
    try {
      event.currentTarget.releasePointerCapture(event.pointerId);
    } catch {
      // Pointer may already be released.
    }
    pendingStrokeRef.current = null;

    const finalized =
      pending.points.length > 1
        ? pending
        : { ...pending, points: [...pending.points, pending.points[0]] };
    const next = [...canvasStrokes, finalized];
    setCanvasStrokes(next);
    pushCanvasState(next);
  };

  const handleCanvasUndo = () => {
    if (!canEdit || canvasStrokes.length === 0) return;
    const next = canvasStrokes.slice(0, -1);
    setCanvasStrokes(next);
    pushCanvasState(next);
  };

  const handleCanvasClear = () => {
    if (!canEdit) return;
    setCanvasStrokes([]);
    pushCanvasState([]);
  };

  const handleDownloadText = (format: 'txt' | 'md') => {
    const ext = format === 'md' ? 'md' : 'txt';
    const mime =
      format === 'md' ? 'text/markdown;charset=utf-8' : 'text/plain;charset=utf-8';
    const plain = pagesToPlainText(pages);
    downloadBlob(new Blob([plain], { type: mime }), `${sanitizeDocumentName(documentName)}.${ext}`);
  };

  const handleDownloadPdf = () => {
    const plain = pagesToPlainText(pages);
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
      setToolAndBroadcast('text');

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

      commitDocument([page], pageSize, true);
      setActivePageId(page.id);
      setLocalMessage(`Loaded ${file.name}`);
    } catch (err: any) {
      setLocalMessage(err?.message || 'Failed to import file');
    } finally {
      event.target.value = '';
    }
  };

  const metrics = PAGE_SIZES[pageSize];

  return (
    <section className="panel space-y-4 p-5 sm:p-6">
      <div className="space-y-2">
        <h2 className="text-xl font-semibold">Create Together</h2>
        <p className="text-sm muted">
          Collaborative paged documents + shared canvas. Use rich text controls and add pages like a standard document editor.
        </p>
      </div>

      <div className="flex flex-wrap gap-2">
        <button
          type="button"
          className={`px-4 py-2 text-sm rounded-lg ${activeTool === 'text' ? 'btn-primary' : 'btn-secondary'}`}
          onClick={() => setToolAndBroadcast('text')}
        >
          Document
        </button>
        <button
          type="button"
          className={`px-4 py-2 text-sm rounded-lg ${activeTool === 'canvas' ? 'btn-primary' : 'btn-secondary'}`}
          onClick={() => setToolAndBroadcast('canvas')}
        >
          Canvas
        </button>
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
            <label className="text-xs uppercase tracking-wide muted">Page Size</label>
            <select
              className="select px-3 py-2 text-sm"
              value={pageSize}
              onChange={(event) => handlePageSizeChange(event.target.value as PageSize)}
              disabled={!canEdit}
            >
              <option value="a4">A4</option>
              <option value="letter">Letter</option>
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
            <button
              type="button"
              className={commandButtonClass(false)}
              onClick={() => applyCommandToSelection('bold')}
              disabled={!canEdit}
            >
              Bold
            </button>
            <button
              type="button"
              className={commandButtonClass(false)}
              onClick={() => applyCommandToSelection('italic')}
              disabled={!canEdit}
            >
              Italic
            </button>
            <button
              type="button"
              className={commandButtonClass(false)}
              onClick={() => applyCommandToSelection('justifyLeft')}
              disabled={!canEdit}
            >
              Left
            </button>
            <button
              type="button"
              className={commandButtonClass(false)}
              onClick={() => applyCommandToSelection('justifyCenter')}
              disabled={!canEdit}
            >
              Center
            </button>
            <button
              type="button"
              className={commandButtonClass(false)}
              onClick={() => applyCommandToSelection('justifyRight')}
              disabled={!canEdit}
            >
              Right
            </button>
            <button
              type="button"
              className={commandButtonClass(false)}
              onClick={() => applyCommandToSelection('insertUnorderedList')}
              disabled={!canEdit}
            >
              Bullets
            </button>
            <button
              type="button"
              className={commandButtonClass(false)}
              onClick={() => applyCommandToSelection('insertOrderedList')}
              disabled={!canEdit}
            >
              Numbered
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
                          pageRefs.current[page.id] = node;
                        }}
                        contentEditable={canEdit}
                        suppressContentEditableWarning
                        className="outline-none px-14 py-14 text-[15px] leading-7"
                        style={{ minHeight: `${metrics.heightPx - 112}px` }}
                        dangerouslySetInnerHTML={{ __html: page.html }}
                        onFocus={() => setActivePageId(page.id)}
                        onInput={(event) => {
                          const html = (event.currentTarget as HTMLDivElement).innerHTML;
                          const sanitized = sanitizePageHtml(html);
                          setPages((prev) => {
                            const next = prev.map((entry) =>
                              entry.id === page.id
                                ? {
                                    ...entry,
                                    html: sanitized,
                                  }
                                : entry,
                            );
                            scheduleDocumentSync(next, pageSize);
                            return next;
                          });
                        }}
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
              onClick={handleCanvasClear}
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
          </div>

          <div className="overflow-hidden rounded-xl border border-white/10 bg-black/40">
            <canvas
              ref={canvasRef}
              width={1000}
              height={560}
              className="h-auto w-full touch-none"
              onPointerDown={onCanvasPointerDown}
              onPointerMove={onCanvasPointerMove}
              onPointerUp={finishStroke}
              onPointerLeave={finishStroke}
            />
          </div>
          <p className="text-xs muted">{canvasMessage}</p>
        </div>
      )}

      {localMessage && <div className="notice-ok rounded-xl px-3 py-2 text-xs">{localMessage}</div>}
    </section>
  );
}

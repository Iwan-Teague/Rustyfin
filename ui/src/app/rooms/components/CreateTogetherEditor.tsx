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

const MAX_DOC_NAME = 120;

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

export default function CreateTogetherEditor({ createState, canEdit, sendWs }: Props) {
  const [activeTool, setActiveTool] = useState<ActiveTool>('text');
  const [documentName, setDocumentName] = useState('Untitled Document');
  const [textFormat, setTextFormat] = useState<TextFormat>('plain');
  const [textContent, setTextContent] = useState('');
  const [canvasStrokes, setCanvasStrokes] = useState<WsCreateCanvasStroke[]>([]);
  const [brushColor, setBrushColor] = useState('#b95cff');
  const [brushSize, setBrushSize] = useState(4);
  const [localMessage, setLocalMessage] = useState('');

  const canvasRef = useRef<HTMLCanvasElement | null>(null);
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

  useEffect(() => {
    if (!createState) return;
    if (createState.updated_ts_ms < latestUpdateRef.current) return;
    latestUpdateRef.current = createState.updated_ts_ms;

    setActiveTool(createState.active_tool === 'canvas' ? 'canvas' : 'text');
    setDocumentName(sanitizeDocumentName(createState.document_name || 'Untitled Document'));

    if (
      createState.text_format === 'markdown' ||
      createState.text_format === 'pdf_text'
    ) {
      setTextFormat(createState.text_format);
    } else {
      setTextFormat('plain');
    }

    setTextContent(createState.text_content || '');
    setCanvasStrokes(createState.canvas_strokes || []);
  }, [createState]);

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

  const pushCanvasState = useCallback(
    (strokes: WsCreateCanvasStroke[]) => {
      if (!canEdit) return;
      sendWs({
        type: 'create_set_canvas',
        canvas_strokes: strokes,
      });
    },
    [canEdit, sendWs],
  );

  const pushTextState = useCallback(
    (nextText: string, nextFormat: TextFormat) => {
      if (!canEdit) return;
      sendWs({
        type: 'create_set_text',
        text_content: nextText,
        text_format: nextFormat,
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

  const onTextChange = (next: string) => {
    setTextContent(next);
    if (!canEdit) return;
    if (textDebounceRef.current) window.clearTimeout(textDebounceRef.current);
    textDebounceRef.current = window.setTimeout(() => {
      pushTextState(next, textFormat);
    }, 220);
  };

  const onNameChange = (next: string) => {
    setDocumentName(next.slice(0, MAX_DOC_NAME));
    if (!canEdit) return;
    if (nameDebounceRef.current) window.clearTimeout(nameDebounceRef.current);
    nameDebounceRef.current = window.setTimeout(() => {
      pushDocumentName(next);
    }, 280);
  };

  const onTextFormatChange = (next: TextFormat) => {
    setTextFormat(next);
    if (!canEdit) return;
    pushTextState(textContent, next);
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

    const finalized = pending.points.length > 1 ? pending : { ...pending, points: [...pending.points, pending.points[0]] };
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
    const mime = format === 'md' ? 'text/markdown;charset=utf-8' : 'text/plain;charset=utf-8';
    downloadBlob(new Blob([textContent], { type: mime }), `${sanitizeDocumentName(documentName)}.${ext}`);
  };

  const handleDownloadPdf = () => {
    const bytes = buildSimplePdfBytes(textContent, sanitizeDocumentName(documentName));
    const pdfBuffer = bytes.buffer.slice(
      bytes.byteOffset,
      bytes.byteOffset + bytes.byteLength,
    ) as ArrayBuffer;
    downloadBlob(new Blob([pdfBuffer], { type: 'application/pdf' }), `${sanitizeDocumentName(documentName)}.pdf`);
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
      let nextText = '';
      let nextFormat: TextFormat = 'plain';

      if (lower.endsWith('.pdf')) {
        const buffer = await file.arrayBuffer();
        nextText = extractPdfText(buffer);
        nextFormat = 'pdf_text';
      } else {
        nextText = await file.text();
        nextFormat = lower.endsWith('.md') ? 'markdown' : 'plain';
      }

      setTextFormat(nextFormat);
      setTextContent(nextText);
      pushTextState(nextText, nextFormat);
      setLocalMessage(`Loaded ${file.name}`);
    } catch (err: any) {
      setLocalMessage(err?.message || 'Failed to import file');
    } finally {
      event.target.value = '';
    }
  };

  return (
    <section className="panel space-y-4 p-5 sm:p-6">
      <div className="space-y-2">
        <h2 className="text-xl font-semibold">Create Together</h2>
        <p className="text-sm muted">
          Shared docs + shared canvas. Export as TXT/MD/PDF/PNG. PDF import extracts text for collaborative editing.
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
            <select
              className="select px-3 py-2 text-sm"
              value={textFormat}
              onChange={(event) => onTextFormatChange(event.target.value as TextFormat)}
              disabled={!canEdit}
            >
              <option value="plain">Plain Text</option>
              <option value="markdown">Markdown</option>
              <option value="pdf_text">PDF Text</option>
            </select>

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

          <textarea
            value={textContent}
            onChange={(event) => onTextChange(event.target.value)}
            className="input min-h-[22rem] w-full resize-y px-3 py-2 text-sm font-mono leading-6"
            placeholder="Start writing together..."
            disabled={!canEdit}
          />
          <p className="text-xs muted">
            {canEdit
              ? 'Edits sync in realtime for everyone in the room.'
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

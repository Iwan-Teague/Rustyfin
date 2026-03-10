'use client';

import { useEffect, useRef, useState, useCallback } from 'react';
import type {
  ChannelEvent,
  ChannelInfo,
  ChannelMessage,
  ChannelMessageAttachment,
} from '@/lib/channelsApi';
import { deleteMessage, getMessages, uploadMessageAttachment } from '@/lib/channelsApi';
import { apiFetch } from '@/lib/api';
import { findDataDeleteTarget, playTelegramDeleteAnimation } from '@/lib/deleteAnimation';
import { clientErrorMessage } from '@/lib/errors';
import { useListReflowAnimation } from '@/lib/listReflowAnimation';
import ConfirmModal from '@/app/components/ConfirmModal';

const DELETE_AFTER_CONFIRM_DELAY_MS = 500;

interface Props {
  channel: ChannelInfo;
  newMessages: ChannelMessage[];
  currentUserId: string;
  isAdmin: boolean;
  wsEvents: ChannelEvent | null;
  onSendMessage: (content: string) => Promise<ChannelMessage | null>;
}

function relativeTime(ts: number): string {
  const diff = Math.floor(Date.now() / 1000) - ts;
  if (diff < 60) return 'just now';
  if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
  return `${Math.floor(diff / 86400)}d ago`;
}

function hashColor(userId: string): string {
  let hash = 0;
  for (let i = 0; i < userId.length; i++) {
    hash = userId.charCodeAt(i) + ((hash << 5) - hash);
  }
  const colors = [
    '#e67e22', '#3498db', '#2ecc71', '#9b59b6', '#e74c3c',
    '#1abc9c', '#f39c12', '#16a085', '#d35400', '#8e44ad',
  ];
  return colors[Math.abs(hash) % colors.length];
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function isImageAttachment(contentType: string): boolean {
  return contentType.toLowerCase().startsWith('image/');
}

function AttachmentPreview({
  attachment,
  onDownload,
}: {
  attachment: ChannelMessageAttachment;
  onDownload: (attachment: ChannelMessageAttachment) => void;
}) {
  const [imageUrl, setImageUrl] = useState<string | null>(null);
  const [previewError, setPreviewError] = useState<string | null>(null);

  useEffect(() => {
    if (!isImageAttachment(attachment.content_type)) return;
    let active = true;
    let objectUrl: string | null = null;

    (async () => {
      try {
        const res = await apiFetch(attachment.download_path, { method: 'GET' });
        if (!res.ok) {
          throw new Error(`HTTP ${res.status}`);
        }
        const blob = await res.blob();
        if (!active) return;
        objectUrl = URL.createObjectURL(blob);
        setImageUrl(objectUrl);
      } catch {
        if (!active) return;
        setPreviewError('Preview unavailable');
      }
    })();

    return () => {
      active = false;
      if (objectUrl) {
        URL.revokeObjectURL(objectUrl);
      }
    };
  }, [attachment.content_type, attachment.download_path]);

  if (isImageAttachment(attachment.content_type) && imageUrl && !previewError) {
    return (
      <div className="space-y-1">
        <img
          src={imageUrl}
          alt={attachment.filename}
          className="max-h-64 rounded-lg border border-[var(--border)] bg-black/20 object-contain"
          loading="lazy"
        />
        <button
          type="button"
          className="btn-ghost px-2 py-1 text-xs"
          onClick={() => onDownload(attachment)}
        >
          Download {attachment.filename}
        </button>
      </div>
    );
  }

  return (
    <div className="panel-soft rounded-lg px-3 py-2 text-xs flex items-center justify-between gap-2">
      <div className="min-w-0">
        <p className="truncate font-medium">{attachment.filename}</p>
        <p className="muted">
          {attachment.content_type} · {formatBytes(attachment.size_bytes)}
        </p>
      </div>
      <button
        type="button"
        className="btn-secondary px-2 py-1 text-xs shrink-0"
        onClick={() => onDownload(attachment)}
      >
        Download
      </button>
    </div>
  );
}

function Avatar({
  userId,
  username,
  avatarUrl,
}: {
  userId: string;
  username: string;
  avatarUrl?: string | null;
}) {
  const color = hashColor(userId);
  const initials = username.slice(0, 2).toUpperCase();
  if (avatarUrl) {
    return (
      <img
        src={avatarUrl}
        alt={username}
        className="w-8 h-8 rounded-full object-cover shrink-0 border border-[var(--border)] bg-black/20"
        loading="lazy"
      />
    );
  }
  return (
    <div
      className="w-8 h-8 rounded-full flex items-center justify-center text-xs font-bold text-white shrink-0"
      style={{ backgroundColor: color }}
    >
      {initials}
    </div>
  );
}

export default function TextChannelView({ channel, newMessages, currentUserId, isAdmin, wsEvents, onSendMessage }: Props) {
  const [messages, setMessages] = useState<ChannelMessage[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [hasMore, setHasMore] = useState(true);
  const [draft, setDraft] = useState('');
  const [pendingFile, setPendingFile] = useState<File | null>(null);
  const [uploading, setUploading] = useState(false);
  const [uploadError, setUploadError] = useState<string | null>(null);
  const [pendingDeleteId, setPendingDeleteId] = useState<string | null>(null);
  const topSentinelRef = useRef<HTMLDivElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);

  // Load initial messages
  useEffect(() => {
    setLoading(true);
    setMessages([]);
    setHasMore(true);
    setPendingFile(null);
    setUploadError(null);
    getMessages(channel.id).then((msgs) => {
      setMessages(msgs);
      setHasMore(msgs.length >= 50);
      setLoading(false);
    });
  }, [channel.id]);

  // Append real-time messages
  useEffect(() => {
    if (newMessages.length === 0) return;
    const relevant = newMessages.filter((m) => m.channel_id === channel.id);
    if (relevant.length === 0) return;
    setMessages((prev) => {
      const existing = new Set(prev.map((m) => m.id));
      const toAdd = relevant.filter((m) => !existing.has(m.id));
      return [...prev, ...toAdd];
    });
  }, [newMessages, channel.id]);

  // Remove deleted messages in real time
  useEffect(() => {
    if (!wsEvents || wsEvents.type !== 'message_deleted') return;
    if (wsEvents.channel_id !== channel.id) return;
    setMessages((prev) => prev.filter((m) => m.id !== wsEvents.message_id));
  }, [wsEvents, channel.id]);

  const handleDeleteMessage = useCallback(async (messageId: string) => {
    setPendingDeleteId(null);
    await new Promise<void>((resolve) => {
      window.setTimeout(resolve, DELETE_AFTER_CONFIRM_DELAY_MS);
    });
    try {
      const target = findDataDeleteTarget('data-delete-message-id', messageId);
      await playTelegramDeleteAnimation(target);
      await deleteMessage(channel.id, messageId);
      setMessages((prev) => prev.filter((m) => m.id !== messageId));
    } catch {
      // Silently ignore — the button simply won't work if permissions mismatch
    }
  }, [channel.id]);

  const handleDownloadAttachment = useCallback(async (attachment: ChannelMessageAttachment) => {
    try {
      const res = await apiFetch(attachment.download_path, { method: 'GET' });
      if (!res.ok) {
        throw new Error(`HTTP ${res.status}`);
      }
      const blob = await res.blob();
      const objectUrl = URL.createObjectURL(blob);
      const anchor = document.createElement('a');
      anchor.href = objectUrl;
      anchor.download = attachment.filename;
      anchor.rel = 'noopener noreferrer';
      document.body.appendChild(anchor);
      anchor.click();
      anchor.remove();
      window.setTimeout(() => URL.revokeObjectURL(objectUrl), 1000);
    } catch {
      setUploadError(`Failed to download ${attachment.filename}`);
    }
  }, []);

  // Auto-scroll to bottom on new messages — scroll the container, not the page
  useEffect(() => {
    const el = listRef.current;
    if (!el) return;
    el.scrollTop = el.scrollHeight;
  }, [messages]);

  useListReflowAnimation(listRef, messages.map((msg) => msg.id), {
    itemSelector: '[data-list-item-id]',
  });

  // IntersectionObserver for load-more at top
  useEffect(() => {
    if (!topSentinelRef.current) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries[0].isIntersecting && hasMore && !loadingMore && messages.length > 0) {
          loadMore();
        }
      },
      { threshold: 0.1 },
    );
    observer.observe(topSentinelRef.current);
    return () => observer.disconnect();
  });

  const loadMore = useCallback(async () => {
    if (loadingMore || !hasMore || messages.length === 0) return;
    const oldest = messages[0];
    setLoadingMore(true);
    try {
      const older = await getMessages(channel.id, oldest.created_ts, oldest.id, 50);
      setHasMore(older.length >= 50);
      if (older.length > 0) {
        setMessages((prev) => [...older, ...prev]);
      }
    } finally {
      setLoadingMore(false);
    }
  }, [channel.id, loadingMore, hasMore, messages]);

  const handleUploadAttachment = useCallback(async () => {
    if (!pendingFile || uploading) return;
    setUploading(true);
    setUploadError(null);
    try {
      const sent = await uploadMessageAttachment(
        channel.id,
        pendingFile,
        draft.trim() ? draft.trim() : undefined,
      );
      setMessages((prev) => {
        if (prev.some((m) => m.id === sent.id)) return prev;
        return [...prev, sent];
      });
      setDraft('');
      setPendingFile(null);
      if (fileInputRef.current) {
        fileInputRef.current.value = '';
      }
    } catch (error: unknown) {
      setUploadError(clientErrorMessage(error, 'Failed to upload file'));
    } finally {
      setUploading(false);
    }
  }, [channel.id, draft, pendingFile, uploading]);

  const handleSend = useCallback(async () => {
    if (uploading) return;
    if (pendingFile) {
      await handleUploadAttachment();
      return;
    }
    const trimmed = draft.trim();
    if (!trimmed) return;
    try {
      const sent = await onSendMessage(trimmed);
      if (sent) {
        setMessages((prev) => {
          if (prev.some((message) => message.id === sent.id)) {
            return prev;
          }
          return [...prev, sent];
        });
      }
      setDraft('');
    } catch (error: unknown) {
      setUploadError(clientErrorMessage(error, 'Failed to send message'));
    }
  }, [draft, handleUploadAttachment, onSendMessage, pendingFile, uploading]);

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      void handleSend();
    }
  };

  // Group consecutive messages from same user
  const grouped = messages.reduce<{ showHeader: boolean; msg: ChannelMessage }[]>((acc, msg, i) => {
    const prev = messages[i - 1];
    const showHeader =
      !prev ||
      prev.user_id !== msg.user_id ||
      msg.created_ts - prev.created_ts > 300;
    acc.push({ showHeader, msg });
    return acc;
  }, []);

  return (
    <div className="flex flex-col flex-1 h-full overflow-hidden">
      {/* Header */}
      <div className="h-14 px-4 border-b border-[var(--border)] flex items-center gap-2 shrink-0">
        <span className="muted">#</span>
        <span className="font-semibold">{channel.name}</span>
      </div>

      {/* Message list — min-h-0 lets flex-1 shrink so the input stays pinned */}
      <div ref={listRef} className="flex-1 min-h-0 overflow-y-auto px-4 py-2 space-y-0.5">
        <div ref={topSentinelRef} className="h-1" />
        {loadingMore && (
          <p className="text-xs muted text-center py-2">Loading older messages…</p>
        )}
        {loading && (
          <p className="text-xs muted text-center py-8">Loading…</p>
        )}
        {!loading && messages.length === 0 && (
          <p className="text-xs muted text-center py-8">
            No messages yet. Say something!
          </p>
        )}

        {grouped.map(({ showHeader, msg }) => {
          const canDelete = isAdmin || msg.user_id === currentUserId;
          const attachments = msg.attachments || [];
          return (
            <div
              key={msg.id}
              data-delete-message-id={msg.id}
              data-list-item-id={msg.id}
              className={`group relative ${showHeader ? 'mt-3' : 'mt-0.5'}`}
            >
              {showHeader ? (
                <div className="flex items-start gap-3 pr-8">
                  <Avatar
                    userId={msg.user_id}
                    username={msg.username}
                    avatarUrl={msg.avatar_url}
                  />
                  <div className="min-w-0 space-y-1">
                    <div className="flex items-baseline gap-2">
                      <span className="font-semibold text-sm">{msg.username}</span>
                      <span className="text-xs muted">{relativeTime(msg.created_ts)}</span>
                    </div>
                    {msg.content && (
                      <p className="text-sm whitespace-pre-wrap break-words">{msg.content}</p>
                    )}
                    {attachments.length > 0 && (
                      <div className="space-y-2">
                        {attachments.map((attachment) => (
                          <AttachmentPreview
                            key={attachment.id}
                            attachment={attachment}
                            onDownload={handleDownloadAttachment}
                          />
                        ))}
                      </div>
                    )}
                  </div>
                </div>
              ) : (
                <div className="pl-11 pr-8 space-y-1">
                  {msg.content && (
                    <div className="text-sm whitespace-pre-wrap break-words">{msg.content}</div>
                  )}
                  {attachments.length > 0 && (
                    <div className="space-y-2">
                      {attachments.map((attachment) => (
                        <AttachmentPreview
                          key={attachment.id}
                          attachment={attachment}
                          onDownload={handleDownloadAttachment}
                        />
                      ))}
                    </div>
                  )}
                </div>
              )}
              {canDelete && (
                <button
                  onClick={() => setPendingDeleteId(msg.id)}
                  className="absolute right-1 top-0 opacity-0 group-hover:opacity-100 btn-ghost px-1.5 py-0.5 text-xs text-red-400 hover:text-red-300 transition-opacity"
                  title="Delete message"
                >
                  ✕
                </button>
              )}
            </div>
          );
        })}
      </div>

      {/* Input */}
      <div className="min-h-16 px-4 py-3 border-t border-[var(--border)] shrink-0">
        <div className="flex items-stretch gap-2">
          <input
            ref={fileInputRef}
            type="file"
            className="hidden"
            onChange={(e) => {
              const selected = e.target.files?.[0] ?? null;
              setPendingFile(selected);
              setUploadError(null);
            }}
          />
          <button
            type="button"
            className="btn-primary h-10 w-10 shrink-0 text-xl leading-none"
            onClick={() => fileInputRef.current?.click()}
            aria-label="Attach file"
            title="Attach file"
            disabled={uploading}
          >
            +
          </button>
          <textarea
            className="panel h-10 flex-1 resize-none overflow-y-auto rounded-lg px-3 py-2 text-sm"
            rows={1}
            placeholder={`Message #${channel.name}`}
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={handleKeyDown}
            maxLength={2000}
            disabled={uploading}
          />
          <button
            type="button"
            className="btn-primary h-10 px-4 text-sm shrink-0 disabled:opacity-60"
            onClick={() => void handleSend()}
            disabled={uploading || (!draft.trim() && !pendingFile)}
          >
            {uploading ? 'Sending…' : 'Send'}
          </button>
        </div>
        {pendingFile && (
          <div className="mt-2 flex items-center gap-2">
            <div className="panel-soft rounded-md px-2 py-1 text-xs max-w-[22rem] truncate">
              {pendingFile.name} · {formatBytes(pendingFile.size)}
            </div>
            {!uploading && (
              <button
                type="button"
                className="btn-ghost px-2 py-1 text-xs"
                onClick={() => {
                  setPendingFile(null);
                  if (fileInputRef.current) {
                    fileInputRef.current.value = '';
                  }
                }}
              >
                Clear
              </button>
            )}
          </div>
        )}
        {uploadError && <p className="text-xs text-red-400 mt-1">{uploadError}</p>}
      </div>

      <ConfirmModal
        open={Boolean(pendingDeleteId)}
        title="Delete Message"
        description="This message will be permanently removed for everyone and cannot be undone."
        confirmLabel="Delete"
        destructive
        onCancel={() => setPendingDeleteId(null)}
        onConfirm={() => {
          if (!pendingDeleteId) return;
          void handleDeleteMessage(pendingDeleteId);
        }}
      />
    </div>
  );
}

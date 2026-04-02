import { useEffect, useRef, useState } from 'react';

import { type AiConversationSummary } from '@/lib/aiApi';

function formatUpdated(updatedTs: number): string {
  const date = new Date(updatedTs * 1000);
  const now = Date.now();
  const diffMs = now - date.getTime();
  const diffMinutes = Math.floor(diffMs / 60000);
  const diffHours = Math.floor(diffMs / 3600000);
  const diffDays = Math.floor(diffMs / 86400000);

  if (diffMinutes < 1) return 'now';
  if (diffMinutes < 60) return `${diffMinutes}m`;
  if (diffHours < 24) return `${diffHours}h`;
  if (diffDays < 7) return `${diffDays}d`;
  return date.toLocaleDateString();
}

function ConversationRow({
  conversation,
  active,
  disabled,
  archiveLabel,
  onSelect,
  onRename,
  onArchive,
  onDelete,
}: {
  conversation: AiConversationSummary;
  active: boolean;
  disabled: boolean;
  archiveLabel: string;
  onSelect: () => void;
  onRename: () => void;
  onArchive: () => void;
  onDelete: () => void;
}) {
  const menuRef = useRef<HTMLDivElement | null>(null);
  const [menuOpen, setMenuOpen] = useState(false);

  useEffect(() => {
    if (!menuOpen) return;
    const handlePointerDown = (event: PointerEvent) => {
      if (!menuRef.current?.contains(event.target as Node)) {
        setMenuOpen(false);
      }
    };
    document.addEventListener('pointerdown', handlePointerDown);
    return () => document.removeEventListener('pointerdown', handlePointerDown);
  }, [menuOpen]);

  const runAction = (callback: () => void) => {
    setMenuOpen(false);
    callback();
  };

  return (
    <div
      data-ai-conversation-row-id={conversation.id}
      className="relative border-b transition-colors last:border-b-0"
      style={{
        borderColor: 'var(--border)',
        background: active
          ? 'linear-gradient(135deg, rgba(255,145,77,0.08), rgba(157,116,255,0.08))'
          : 'transparent',
      }}
    >
      <div className="flex items-start gap-2 px-3.5 py-3">
        <button
          type="button"
          onClick={onSelect}
          disabled={disabled}
          className="min-w-0 flex-1 text-left disabled:opacity-60"
        >
          <div className="flex items-start justify-between gap-3">
            <div className="min-w-0 truncate text-sm font-medium text-[var(--text-main)]">
              {conversation.title}
            </div>
            <span className="shrink-0 pt-0.5 text-[0.64rem] muted">
              {formatUpdated(conversation.updated_ts)}
            </span>
          </div>
        </button>

        <div ref={menuRef} className="relative shrink-0">
          <button
            type="button"
            onClick={() => setMenuOpen((current) => !current)}
            disabled={disabled}
            className="btn-ghost flex h-8 w-8 items-center justify-center rounded-full p-0 text-lg leading-none disabled:opacity-40"
            aria-label="Conversation actions"
          >
            ⋯
          </button>
          {menuOpen ? (
            <div className="absolute right-0 top-[calc(100%+0.35rem)] z-20 min-w-[9rem] overflow-hidden rounded-xl border border-[var(--border)] bg-[rgba(16,20,31,0.98)] shadow-[0_18px_44px_rgba(0,0,0,0.34)]">
              <button
                type="button"
                onClick={() => runAction(onRename)}
                className="block w-full px-3 py-2 text-left text-xs text-[var(--text-main)] transition-colors hover:bg-[rgba(255,255,255,0.04)]"
              >
                Rename
              </button>
              <button
                type="button"
                onClick={() => runAction(onArchive)}
                className="block w-full px-3 py-2 text-left text-xs text-[var(--text-main)] transition-colors hover:bg-[rgba(255,255,255,0.04)]"
              >
                {archiveLabel}
              </button>
              <button
                type="button"
                onClick={() => runAction(onDelete)}
                className="block w-full px-3 py-2 text-left text-xs text-[var(--danger)] transition-colors hover:bg-[rgba(255,255,255,0.04)]"
              >
                Delete
              </button>
            </div>
          ) : null}
        </div>
      </div>
    </div>
  );
}

export default function AiConversationRail({
  conversations,
  archivedConversations,
  activeConversationId,
  disabled,
  className = '',
  onSelect,
  onNewChat,
  onRename,
  onArchiveToggle,
  onDelete,
}: {
  conversations: AiConversationSummary[];
  archivedConversations: AiConversationSummary[];
  activeConversationId: string | null;
  disabled: boolean;
  className?: string;
  onSelect: (conversationId: string) => void;
  onNewChat: () => void;
  onRename: (conversation: AiConversationSummary) => void;
  onArchiveToggle: (conversation: AiConversationSummary) => void;
  onDelete: (conversation: AiConversationSummary) => void;
}) {
  return (
    <aside
      className={`flex min-h-0 w-[min(19rem,88vw)] flex-col bg-transparent sm:w-[19rem] ${className}`}
    >
      <div className="shrink-0 border-b border-[var(--border)] px-4 py-4">
        <button
          type="button"
          onClick={onNewChat}
          disabled={disabled}
          className="btn-primary w-full rounded-xl px-4 py-2.5 text-sm disabled:opacity-40"
        >
          New chat
        </button>
      </div>

      <div className="flex-1 space-y-5 overflow-y-auto px-3 py-4">
        <section className="space-y-2.5">
          <div className="px-1 text-[0.68rem] font-semibold uppercase tracking-[0.18em] text-[var(--text-faint)]">
            Recent
          </div>
          {conversations.length === 0 ? (
            <div className="border border-dashed border-[var(--border)] px-4 py-6 text-center text-[0.75rem] muted">
              No saved chats yet
            </div>
          ) : (
            conversations.map((conversation) => (
              <ConversationRow
                key={conversation.id}
                conversation={conversation}
                active={conversation.id === activeConversationId}
                disabled={disabled}
                archiveLabel="Archive"
                onSelect={() => onSelect(conversation.id)}
                onRename={() => onRename(conversation)}
                onArchive={() => onArchiveToggle(conversation)}
                onDelete={() => onDelete(conversation)}
              />
            ))
          )}
        </section>

        {archivedConversations.length > 0 && (
          <section className="space-y-2.5">
            <div className="px-1 text-[0.68rem] font-semibold uppercase tracking-[0.18em] text-[var(--text-faint)]">
              Archived
            </div>
            {archivedConversations.map((conversation) => (
              <ConversationRow
                key={conversation.id}
                conversation={conversation}
                active={conversation.id === activeConversationId}
                disabled={disabled}
                archiveLabel="Restore"
                onSelect={() => onSelect(conversation.id)}
                onRename={() => onRename(conversation)}
                onArchive={() => onArchiveToggle(conversation)}
                onDelete={() => onDelete(conversation)}
              />
            ))}
          </section>
        )}
      </div>
    </aside>
  );
}

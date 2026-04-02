import { useEffect, useRef, useState } from 'react';

import { type AiConversationSummary } from '@/lib/aiApi';

type ConversationGroup = {
  id: string;
  title: string;
  items: AiConversationSummary[];
  archiveLabel: string;
};

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
    <div data-ai-conversation-row-id={conversation.id} className="relative px-1">
      <div
        className={`flex items-center gap-1.5 rounded-[1.2rem] px-1.5 py-1 transition-all ${
          active
            ? 'bg-[rgba(222,230,255,0.11)] shadow-[inset_0_1px_0_rgba(255,255,255,0.04)]'
            : 'hover:bg-[rgba(255,255,255,0.04)]'
        }`}
      >
        <button
          type="button"
          onClick={onSelect}
          disabled={disabled}
          className="min-w-0 flex-1 rounded-[1rem] px-2.5 py-1.5 text-left disabled:opacity-60"
        >
          <div className="flex items-center justify-between gap-3">
            <div className="min-w-0 truncate text-[0.82rem] font-medium text-[var(--text-main)]">
              {conversation.title}
            </div>
            <span className="shrink-0 text-[0.74rem] muted">
              {formatUpdated(conversation.updated_ts)}
            </span>
          </div>
        </button>

        <div ref={menuRef} className="relative shrink-0">
          <button
            type="button"
            onClick={() => setMenuOpen((current) => !current)}
            disabled={disabled}
            className={`flex h-7 w-7 items-center justify-center rounded-full p-0 text-base leading-none transition-colors disabled:opacity-40 ${
              active
                ? 'bg-[rgba(255,255,255,0.05)] text-[var(--text-main)] hover:bg-[rgba(255,255,255,0.09)]'
                : 'text-[var(--text-muted)] hover:bg-[rgba(255,255,255,0.05)]'
            }`}
            aria-label="Conversation actions"
          >
            ⋯
          </button>
          {menuOpen ? (
            <div className="absolute right-0 top-[calc(100%+0.35rem)] z-20 min-w-[9rem] overflow-hidden rounded-2xl border border-[var(--border)] bg-[rgba(46,53,80,0.96)] shadow-[0_18px_44px_rgba(0,0,0,0.22)] backdrop-blur-xl">
              <button
                type="button"
                onClick={() => runAction(onRename)}
                className="block w-full px-3 py-2 text-left text-xs text-[var(--text-main)] transition-colors hover:bg-[rgba(255,255,255,0.05)]"
              >
                Rename
              </button>
              <button
                type="button"
                onClick={() => runAction(onArchive)}
                className="block w-full px-3 py-2 text-left text-xs text-[var(--text-main)] transition-colors hover:bg-[rgba(255,255,255,0.05)]"
              >
                {archiveLabel}
              </button>
              <button
                type="button"
                onClick={() => runAction(onDelete)}
                className="block w-full px-3 py-2 text-left text-xs text-[var(--danger)] transition-colors hover:bg-[rgba(255,255,255,0.05)]"
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

function ConversationGroupSection({
  group,
  activeConversationId,
  disabled,
  onSelect,
  onRename,
  onArchiveToggle,
  onDelete,
}: {
  group: ConversationGroup;
  activeConversationId: string | null;
  disabled: boolean;
  onSelect: (conversationId: string) => void;
  onRename: (conversation: AiConversationSummary) => void;
  onArchiveToggle: (conversation: AiConversationSummary) => void;
  onDelete: (conversation: AiConversationSummary) => void;
}) {
  return (
    <section className="space-y-1.5">
      <div className="px-2 text-[0.64rem] font-semibold uppercase tracking-[0.18em] text-[var(--text-faint)]">
        {group.title}
      </div>
      <div className="space-y-0.5">
        {group.items.map((conversation) => (
          <ConversationRow
            key={conversation.id}
            conversation={conversation}
            active={conversation.id === activeConversationId}
            disabled={disabled}
            archiveLabel={group.archiveLabel}
            onSelect={() => onSelect(conversation.id)}
            onRename={() => onRename(conversation)}
            onArchive={() => onArchiveToggle(conversation)}
            onDelete={() => onDelete(conversation)}
          />
        ))}
      </div>
    </section>
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
  const groups: ConversationGroup[] = [];
  if (conversations.length > 0) {
    groups.push({
      id: 'recent',
      title: 'Recent',
      items: conversations,
      archiveLabel: 'Archive',
    });
  }
  if (archivedConversations.length > 0) {
    groups.push({
      id: 'archived',
      title: 'Archived',
      items: archivedConversations,
      archiveLabel: 'Restore',
    });
  }

  return (
    <aside
      className={`flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden bg-transparent ${className}`}
    >
      <div className="shrink-0 px-3 pb-4 pt-0">
        <button
          type="button"
          onClick={onNewChat}
          disabled={disabled}
          className="btn-primary w-full rounded-xl px-4 py-2 text-sm disabled:opacity-40"
        >
          New chat
        </button>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain px-3 pb-3 pt-0">
        {groups.length === 0 ? (
          <div className="px-3 py-5 text-center text-[0.78rem] muted">
            No saved chats yet
          </div>
        ) : (
          <div className="space-y-4">
            {groups.map((group) => (
              <ConversationGroupSection
                key={group.id}
                group={group}
                activeConversationId={activeConversationId}
                disabled={disabled}
                onSelect={onSelect}
                onRename={onRename}
                onArchiveToggle={onArchiveToggle}
                onDelete={onDelete}
              />
            ))}
          </div>
        )}
      </div>
    </aside>
  );
}

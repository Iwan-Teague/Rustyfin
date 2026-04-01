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
  return (
    <div
      data-ai-conversation-row-id={conversation.id}
      className="rounded-2xl border transition-colors"
      style={{
        borderColor: active ? 'rgba(255,145,77,0.28)' : 'var(--border)',
        background: active
          ? 'linear-gradient(135deg, rgba(255,145,77,0.11), rgba(157,116,255,0.12))'
          : 'rgba(255,255,255,0.03)',
      }}
    >
      <button
        type="button"
        onClick={onSelect}
        disabled={disabled}
        className="w-full text-left px-3.5 py-3 disabled:opacity-60"
      >
        <div className="flex items-start justify-between gap-3">
          <div className="min-w-0">
            <div className="truncate text-sm font-medium text-[var(--text-main)]">
              {conversation.title}
            </div>
            <div className="mt-1 truncate text-[0.72rem] muted">
              {conversation.last_message_preview ?? 'No messages yet'}
            </div>
          </div>
          <span className="shrink-0 text-[0.64rem] muted">
            {formatUpdated(conversation.updated_ts)}
          </span>
        </div>
      </button>

      <div className="flex items-center gap-2 px-3.5 pb-3">
        <button
          type="button"
          onClick={onRename}
          disabled={disabled}
          className="text-[0.64rem] muted hover:text-[var(--text-main)] disabled:opacity-40"
        >
          Rename
        </button>
        <button
          type="button"
          onClick={onArchive}
          disabled={disabled}
          className="text-[0.64rem] muted hover:text-[var(--text-main)] disabled:opacity-40"
        >
          {archiveLabel}
        </button>
        <button
          type="button"
          onClick={onDelete}
          disabled={disabled}
          className="text-[0.64rem] text-[var(--danger)] disabled:opacity-40"
        >
          Delete
        </button>
      </div>
    </div>
  );
}

export default function AiConversationRail({
  conversations,
  archivedConversations,
  activeConversationId,
  disabled,
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
  onSelect: (conversationId: string) => void;
  onNewChat: () => void;
  onRename: (conversation: AiConversationSummary) => void;
  onArchiveToggle: (conversation: AiConversationSummary) => void;
  onDelete: (conversation: AiConversationSummary) => void;
}) {
  return (
    <aside className="flex h-full w-[19rem] flex-col border-r border-[var(--border)] bg-[rgba(0,0,0,0.2)]">
      <div className="border-b border-[var(--border)] px-4 py-4">
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
            <div className="rounded-2xl border border-dashed border-[var(--border)] px-4 py-6 text-center text-[0.75rem] muted">
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

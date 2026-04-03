import { useEffect, useRef, useState } from 'react';

import { type AiConversationSummary } from '@/lib/aiApi';

const COLLAPSED_GROUPS_STORAGE_KEY = 'rustyfin-ai-collapsed-groups-v1';

type RailConversationItem = {
  conversation: AiConversationSummary;
  canMoveUp: boolean;
  canMoveDown: boolean;
};

type RailEntry =
  | {
      kind: 'chat';
      item: RailConversationItem;
    }
  | {
      kind: 'group';
      id: string;
      title: string;
      items: RailConversationItem[];
      archived: boolean;
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

function normalizedGroupName(value?: string | null): string | null {
  const normalized = value?.trim();
  return normalized ? normalized : null;
}

function buildRailEntries(
  conversations: AiConversationSummary[],
  archived: boolean,
): RailEntry[] {
  const groupBuckets = new Map<string, AiConversationSummary[]>();
  const ungrouped = conversations.filter(
    (conversation) => !normalizedGroupName(conversation.group_name),
  );

  for (const conversation of conversations) {
    const groupName = normalizedGroupName(conversation.group_name);
    if (!groupName) continue;
    const existing = groupBuckets.get(groupName) ?? [];
    existing.push(conversation);
    groupBuckets.set(groupName, existing);
  }

  const entries: RailEntry[] = [];
  const seenGroups = new Set<string>();

  for (const conversation of conversations) {
    const groupName = normalizedGroupName(conversation.group_name);
    if (!groupName) {
      const index = ungrouped.findIndex((item) => item.id === conversation.id);
      entries.push({
        kind: 'chat',
        item: {
          conversation,
          canMoveUp: index > 0,
          canMoveDown: index >= 0 && index + 1 < ungrouped.length,
        },
      });
      continue;
    }

    if (seenGroups.has(groupName)) {
      continue;
    }
    seenGroups.add(groupName);

    const items = groupBuckets.get(groupName) ?? [];
    entries.push({
      kind: 'group',
      id: `${archived ? 'archived' : 'live'}::${groupName}`,
      title: groupName,
      archived,
      items: items.map((item, index) => ({
        conversation: item,
        canMoveUp: index > 0,
        canMoveDown: index + 1 < items.length,
      })),
    });
  }

  return entries;
}

function ChevronIcon({ open }: { open: boolean }) {
  return (
    <svg
      className={`h-3.5 w-3.5 transition-transform ${open ? 'rotate-90' : ''}`}
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M6 3.5 10.5 8 6 12.5" />
    </svg>
  );
}

function DotsIcon() {
  return (
    <svg
      className="h-4 w-4"
      viewBox="0 0 16 16"
      fill="currentColor"
      aria-hidden="true"
    >
      <circle cx="3" cy="8" r="1.2" />
      <circle cx="8" cy="8" r="1.2" />
      <circle cx="13" cy="8" r="1.2" />
    </svg>
  );
}

function ConversationRow({
  item,
  active,
  disabled,
  archiveLabel,
  indent = false,
  onSelect,
  onRename,
  onMoveToGroup,
  onMoveUp,
  onMoveDown,
  onArchive,
  onDelete,
}: {
  item: RailConversationItem;
  active: boolean;
  disabled: boolean;
  archiveLabel: string;
  indent?: boolean;
  onSelect: () => void;
  onRename: () => void;
  onMoveToGroup: () => void;
  onMoveUp: () => void;
  onMoveDown: () => void;
  onArchive: () => void;
  onDelete: () => void;
}) {
  const { conversation, canMoveUp, canMoveDown } = item;
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
      className={`group relative ${indent ? 'pl-5' : ''}`}
    >
      <div
        className={`flex items-center gap-1.5 rounded-[0.95rem] px-1 py-0.5 transition-colors ${
          active
            ? 'bg-[rgba(222,230,255,0.08)]'
            : 'hover:bg-[rgba(255,255,255,0.03)]'
        }`}
      >
        <button
          type="button"
          onClick={onSelect}
          disabled={disabled}
          className="min-w-0 flex-1 rounded-[0.8rem] px-2.5 py-1.5 text-left disabled:opacity-60"
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
              menuOpen
                ? 'bg-[rgba(255,255,255,0.05)] text-[var(--text-main)]'
                : active
                  ? 'bg-[rgba(255,255,255,0.05)] text-[var(--text-main)] hover:bg-[rgba(255,255,255,0.09)]'
                  : 'text-[var(--text-muted)] opacity-0 group-hover:opacity-100 hover:bg-[rgba(255,255,255,0.05)]'
            }`}
            aria-label="Conversation actions"
          >
            <DotsIcon />
          </button>
          {menuOpen ? (
            <div className="absolute right-0 top-[calc(100%+0.35rem)] z-20 min-w-[10.5rem] overflow-hidden rounded-2xl border border-[var(--border)] bg-[rgba(31,37,54,0.98)] shadow-[0_18px_44px_rgba(0,0,0,0.24)] backdrop-blur-xl">
              <button
                type="button"
                onClick={() => runAction(onRename)}
                className="block w-full px-3 py-2 text-left text-xs text-[var(--text-main)] transition-colors hover:bg-[rgba(255,255,255,0.05)]"
              >
                Rename
              </button>
              <button
                type="button"
                onClick={() => runAction(onMoveToGroup)}
                className="block w-full px-3 py-2 text-left text-xs text-[var(--text-main)] transition-colors hover:bg-[rgba(255,255,255,0.05)]"
              >
                {conversation.group_name ? 'Edit group…' : 'Move to group…'}
              </button>
              <button
                type="button"
                onClick={() => runAction(onMoveUp)}
                disabled={!canMoveUp}
                className="block w-full px-3 py-2 text-left text-xs text-[var(--text-main)] transition-colors hover:bg-[rgba(255,255,255,0.05)] disabled:cursor-not-allowed disabled:opacity-40"
              >
                Move up
              </button>
              <button
                type="button"
                onClick={() => runAction(onMoveDown)}
                disabled={!canMoveDown}
                className="block w-full px-3 py-2 text-left text-xs text-[var(--text-main)] transition-colors hover:bg-[rgba(255,255,255,0.05)] disabled:cursor-not-allowed disabled:opacity-40"
              >
                Move down
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

function GroupEntry({
  entry,
  activeConversationId,
  collapsed,
  disabled,
  onToggle,
  onSelect,
  onRename,
  onMoveToGroup,
  onMoveUp,
  onMoveDown,
  onArchiveToggle,
  onDelete,
}: {
  entry: Extract<RailEntry, { kind: 'group' }>;
  activeConversationId: string | null;
  collapsed: boolean;
  disabled: boolean;
  onToggle: () => void;
  onSelect: (conversationId: string) => void;
  onRename: (conversation: AiConversationSummary) => void;
  onMoveToGroup: (conversation: AiConversationSummary) => void;
  onMoveUp: (conversation: AiConversationSummary) => void;
  onMoveDown: (conversation: AiConversationSummary) => void;
  onArchiveToggle: (conversation: AiConversationSummary) => void;
  onDelete: (conversation: AiConversationSummary) => void;
}) {
  return (
    <div className="space-y-1">
      <button
        type="button"
        onClick={onToggle}
        className="flex w-full items-center gap-2 rounded-[0.95rem] px-2 py-1.5 text-left transition-colors hover:bg-[rgba(255,255,255,0.03)]"
        aria-expanded={!collapsed}
      >
        <span className="shrink-0 text-[var(--text-muted)]">
          <ChevronIcon open={!collapsed} />
        </span>
        <span className="min-w-0 flex-1 truncate text-[0.92rem] font-medium text-[var(--text-main)]">
          {entry.title}
        </span>
        <span className="shrink-0 text-[0.76rem] text-[var(--text-muted)]">
          {entry.items.length}
        </span>
      </button>

      {!collapsed ? (
        <div className="space-y-1">
          {entry.items.map((item) => (
            <ConversationRow
              key={item.conversation.id}
              item={item}
              active={item.conversation.id === activeConversationId}
              disabled={disabled}
              archiveLabel={entry.archived ? 'Restore' : 'Archive'}
              indent
              onSelect={() => onSelect(item.conversation.id)}
              onRename={() => onRename(item.conversation)}
              onMoveToGroup={() => onMoveToGroup(item.conversation)}
              onMoveUp={() => onMoveUp(item.conversation)}
              onMoveDown={() => onMoveDown(item.conversation)}
              onArchive={() => onArchiveToggle(item.conversation)}
              onDelete={() => onDelete(item.conversation)}
            />
          ))}
        </div>
      ) : null}
    </div>
  );
}

function RailSection({
  title,
  entries,
  activeConversationId,
  disabled,
  collapsedGroups,
  onToggleGroup,
  onSelect,
  onRename,
  onMoveToGroup,
  onMoveUp,
  onMoveDown,
  onArchiveToggle,
  onDelete,
}: {
  title: string;
  entries: RailEntry[];
  activeConversationId: string | null;
  disabled: boolean;
  collapsedGroups: Record<string, boolean>;
  onToggleGroup: (groupId: string) => void;
  onSelect: (conversationId: string) => void;
  onRename: (conversation: AiConversationSummary) => void;
  onMoveToGroup: (conversation: AiConversationSummary) => void;
  onMoveUp: (conversation: AiConversationSummary) => void;
  onMoveDown: (conversation: AiConversationSummary) => void;
  onArchiveToggle: (conversation: AiConversationSummary) => void;
  onDelete: (conversation: AiConversationSummary) => void;
}) {
  if (entries.length === 0) return null;

  return (
    <section className="space-y-2">
      <div className="px-2 text-[0.74rem] font-semibold text-[var(--text-muted)]">
        {title}
      </div>
      <div className="space-y-1">
        {entries.map((entry) =>
          entry.kind === 'group' ? (
            <GroupEntry
              key={entry.id}
              entry={entry}
              activeConversationId={activeConversationId}
              collapsed={Boolean(collapsedGroups[entry.id])}
              disabled={disabled}
              onToggle={() => onToggleGroup(entry.id)}
              onSelect={onSelect}
              onRename={onRename}
              onMoveToGroup={onMoveToGroup}
              onMoveUp={onMoveUp}
              onMoveDown={onMoveDown}
              onArchiveToggle={onArchiveToggle}
              onDelete={onDelete}
            />
          ) : (
            <ConversationRow
              key={entry.item.conversation.id}
              item={entry.item}
              active={entry.item.conversation.id === activeConversationId}
              disabled={disabled}
              archiveLabel={entry.item.conversation.archived ? 'Restore' : 'Archive'}
              onSelect={() => onSelect(entry.item.conversation.id)}
              onRename={() => onRename(entry.item.conversation)}
              onMoveToGroup={() => onMoveToGroup(entry.item.conversation)}
              onMoveUp={() => onMoveUp(entry.item.conversation)}
              onMoveDown={() => onMoveDown(entry.item.conversation)}
              onArchive={() => onArchiveToggle(entry.item.conversation)}
              onDelete={() => onDelete(entry.item.conversation)}
            />
          ),
        )}
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
  onMoveToGroup,
  onMoveUp,
  onMoveDown,
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
  onMoveToGroup: (conversation: AiConversationSummary) => void;
  onMoveUp: (conversation: AiConversationSummary) => void;
  onMoveDown: (conversation: AiConversationSummary) => void;
  onArchiveToggle: (conversation: AiConversationSummary) => void;
  onDelete: (conversation: AiConversationSummary) => void;
}) {
  const [collapsedGroups, setCollapsedGroups] = useState<Record<string, boolean>>(() => {
    if (typeof window === 'undefined') {
      return {};
    }
    try {
      const raw = window.localStorage.getItem(COLLAPSED_GROUPS_STORAGE_KEY);
      if (!raw) return {};
      const parsed = JSON.parse(raw);
      return parsed && typeof parsed === 'object'
        ? (parsed as Record<string, boolean>)
        : {};
    } catch {
      return {};
    }
  });

  useEffect(() => {
    if (typeof window === 'undefined') return;
    window.localStorage.setItem(
      COLLAPSED_GROUPS_STORAGE_KEY,
      JSON.stringify(collapsedGroups),
    );
  }, [collapsedGroups]);

  const liveEntries = buildRailEntries(conversations, false);
  const archivedEntries = buildRailEntries(archivedConversations, true);

  const toggleGroup = (groupId: string) => {
    setCollapsedGroups((current) => ({
      ...current,
      [groupId]: !current[groupId],
    }));
  };

  return (
    <aside
      className={`flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden bg-transparent ${className}`}
    >
      <div className="shrink-0 px-3 pb-4 pt-3">
        <button
          type="button"
          onClick={onNewChat}
          disabled={disabled}
          className="w-full rounded-[0.95rem] px-2.5 py-2 text-left text-[0.92rem] font-medium text-[var(--text-main)] transition-colors hover:bg-[rgba(255,255,255,0.03)] disabled:opacity-40"
        >
          New chat
        </button>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain px-3 pb-4">
        {liveEntries.length === 0 && archivedEntries.length === 0 ? (
          <div className="px-3 py-5 text-center text-[0.82rem] muted">
            No saved chats yet
          </div>
        ) : (
          <div className="space-y-5">
            <RailSection
              title="Threads"
              entries={liveEntries}
              activeConversationId={activeConversationId}
              disabled={disabled}
              collapsedGroups={collapsedGroups}
              onToggleGroup={toggleGroup}
              onSelect={onSelect}
              onRename={onRename}
              onMoveToGroup={onMoveToGroup}
              onMoveUp={onMoveUp}
              onMoveDown={onMoveDown}
              onArchiveToggle={onArchiveToggle}
              onDelete={onDelete}
            />
            <RailSection
              title="Archived"
              entries={archivedEntries}
              activeConversationId={activeConversationId}
              disabled={disabled}
              collapsedGroups={collapsedGroups}
              onToggleGroup={toggleGroup}
              onSelect={onSelect}
              onRename={onRename}
              onMoveToGroup={onMoveToGroup}
              onMoveUp={onMoveUp}
              onMoveDown={onMoveDown}
              onArchiveToggle={onArchiveToggle}
              onDelete={onDelete}
            />
          </div>
        )}
      </div>
    </aside>
  );
}

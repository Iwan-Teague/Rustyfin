'use client';

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useRouter } from 'next/navigation';
import { apiJson } from '@/lib/api';
import {
  deleteAiModel,
  fetchAiAuditEvents,
  fetchAiAdminState,
  runAiModelBenchmark,
  type AiAssistantAuditEvent,
  pullAiModelFromUrl,
  type AiAdminState,
  type AdminAiPullEvent,
  updateAiRemoteBackend,
  updateAiModelDir,
} from '@/lib/aiAdminApi';
import { useAuth } from '@/lib/auth';
import { findDataDeleteTarget, playTelegramDeleteAnimation } from '@/lib/deleteAnimation';
import { clientErrorMessage } from '@/lib/errors';
import ConfirmModal from '@/app/components/ConfirmModal';
import {
  listRustyVaultAuditEvents,
  type RustyVaultAuditEventResponse,
} from '@/features/rustyvault/api';
import { ensureRustyVaultWebSession } from '@/features/rustyvault/session';
import {
  listMinecraftServerEvents,
  listMinecraftServerLogs,
  listMinecraftServers,
  MinecraftServer,
  MinecraftServerEvent,
  ServerLogLine,
} from '@/lib/serversApi';

interface Library {
  id: string;
  name: string;
  kind: string;
  paths: { id: string; path: string; is_read_only: boolean }[];
  settings: {
    show_images: boolean;
    prefer_local_artwork: boolean;
    fetch_online_artwork: boolean;
    tmdb_store_in_media_dir: boolean;
    tmdb_sync_on_new_media: boolean;
    tmdb_sync_schedule: 'manual' | 'hourly' | 'daily' | 'weekly' | 'monthly';
    tmdb_last_sync_ts?: number | null;
    tmdb_fetch_posters: boolean;
    tmdb_fetch_backdrops: boolean;
    tmdb_fetch_metadata: boolean;
    tmdb_fetch_reviews: boolean;
  };
  item_count: number;
}

interface LibraryEditState {
  name: string;
  path: string;
  show_images: boolean;
  prefer_local_artwork: boolean;
  fetch_online_artwork: boolean;
  tmdb_store_in_media_dir: boolean;
  tmdb_sync_on_new_media: boolean;
  tmdb_sync_schedule: 'manual' | 'hourly' | 'daily' | 'weekly' | 'monthly';
  tmdb_fetch_posters: boolean;
  tmdb_fetch_backdrops: boolean;
  tmdb_fetch_metadata: boolean;
  tmdb_fetch_reviews: boolean;
}

interface MusicImportState {
  source: string;
  artist: string;
  album: string;
  title: string;
  importing: boolean;
}

interface MusicImportResponse {
  library_id: string;
  video_id: string;
  artist: string;
  album: string;
  title: string;
  file_path: string;
  duration_ms?: number | null;
  scan_job: {
    id: string;
    status: string;
  };
}

interface Job {
  id: string;
  kind: string;
  status: string;
  progress: number;
  payload?: Record<string, unknown> | null;
  error?: string | null;
  created_ts: number;
  updated_ts: number;
}

interface UserAccount {
  id: string;
  username: string;
  role: 'admin' | 'user';
  created_ts: number;
  library_ids: string[];
}

interface UserEditState {
  role: 'admin' | 'user';
  library_ids: string[];
}

interface ChannelRecord {
  id: string;
  name: string;
  kind: 'text' | 'voice';
  position: number;
  is_private: boolean;
  created_by: string;
  created_ts: number;
}

interface ChannelEditState {
  name: string;
  is_private: boolean;
}

interface RoomRecord {
  room_id: string;
  room_name: string;
  title: string;
  host_user_id: string;
  host_username: string;
  item_id: string;
  status: string;
  room_mode: string;
  audio_library_name: string;
  web_url: string;
  password_required: boolean;
  invite_only: boolean;
  member_count: number;
  created_ts: number;
  updated_ts: number;
}

interface RoomEditState {
  room_name: string;
}

interface TmdbConfig {
  configured: boolean;
  key_preview: string | null;
  source: 'database' | 'environment' | null;
}

interface TmdbSyncStatusRow {
  library_id: string;
  library_name: string;
  library_kind: string;
  last_run_result: string;
  last_run_ts: number | null;
  next_scheduled_run_ts: number | null;
  next_scheduled_run_label: string;
  failure_reason: string | null;
}

interface HostDirectoryListEntry {
  name: string;
  path: string;
}

interface HostDirectoryListResponse {
  current_path: string;
  parent_path: string | null;
  roots: string[];
  directories: HostDirectoryListEntry[];
}

interface DiagnosticsCounter {
  enqueued_total: number;
  running_total: number;
  active_running: number;
  completed_total: number;
  failed_total: number;
  failures_last_minute: number;
  failures_last_five_minutes: number;
}

interface DiagnosticsAgentCounter {
  calls_total: number;
  calls_succeeded_total: number;
  calls_failed_total: number;
  calls_in_flight: number;
  failures_last_minute: number;
  failures_last_five_minutes: number;
}

interface RuntimeDiagnosticsResponse {
  host: {
    available: boolean;
    reason: string | null;
    uptime_seconds: number | null;
    logical_cpu_threads: number | null;
    physical_cpu_cores: number | null;
    cpu_usage_percent: number | null;
    estimated_busy_logical_threads: number | null;
    total_memory_bytes: number | null;
    used_memory_bytes: number | null;
    memory_used_percent: number | null;
    total_swap_bytes: number | null;
    used_swap_bytes: number | null;
    swap_used_percent: number | null;
    load_average: {
      one: number;
      five: number;
      fifteen: number;
    } | null;
  };
  runtime: {
    uptime_seconds: number;
    jobs: {
      total: DiagnosticsCounter;
      library_scan: DiagnosticsCounter;
      tmdb_sync: DiagnosticsCounter;
      server_operations: DiagnosticsCounter;
      admin_audit: DiagnosticsCounter;
      other: DiagnosticsCounter;
    };
    websockets: {
      channels: {
        active: number;
        connections_total: number;
      };
      watch_party: {
        active: number;
        connections_total: number;
      };
    };
    agents: {
      servers: DiagnosticsAgentCounter;
      tmdb: DiagnosticsAgentCounter;
      transcription: DiagnosticsAgentCounter;
      youtube: DiagnosticsAgentCounter;
    };
    assistant: {
      chats: DiagnosticsAgentCounter;
      tools: DiagnosticsAgentCounter;
    };
  };
  transcoding: {
    active_sessions: number;
    created_total: number;
    create_failures_total: number;
    create_failures_last_minute: number;
    create_failures_last_five_minutes: number;
    cleaned_total: number;
  };
}

type AdminTab =
  | 'users'
  | 'libraries'
  | 'channels'
  | 'rooms'
  | 'ai'
  | 'server_logs'
  | 'logs'
  | 'vault_audit'
  | 'tmdb';
type LogFilterTab = 'all' | 'complete' | 'failed' | 'in_progress';
type PendingDeleteKind = 'user' | 'library' | 'channel' | 'room';

interface PendingDeleteAction {
  kind: PendingDeleteKind;
  id: string;
  label: string;
}

interface PendingRoomEndAction {
  id: string;
  label: string;
}

interface AiModelPullState {
  status: string;
  percent: number;
  active: boolean;
  done: boolean;
  error: string | null;
}

const ADMIN_TABS: { key: AdminTab; label: string }[] = [
  { key: 'users', label: 'Users' },
  { key: 'libraries', label: 'Libraries' },
  { key: 'channels', label: 'Channels' },
  { key: 'rooms', label: 'Rooms' },
  { key: 'ai', label: 'AI' },
  { key: 'server_logs', label: 'Server Logs' },
  { key: 'logs', label: 'Logs' },
  { key: 'vault_audit', label: 'Vault Audit' },
  { key: 'tmdb', label: 'TMDB Metadata' },
];

const LOG_FILTER_TABS: { key: LogFilterTab; label: string }[] = [
  { key: 'all', label: 'All' },
  { key: 'complete', label: 'Complete' },
  { key: 'failed', label: 'Failed' },
  { key: 'in_progress', label: 'In Progress' },
];

const TMDB_SCHEDULE_INTERVAL_SECONDS: Record<'hourly' | 'daily' | 'weekly' | 'monthly', number> =
  {
    hourly: 60 * 60,
    daily: 60 * 60 * 24,
    weekly: 60 * 60 * 24 * 7,
    monthly: 60 * 60 * 24 * 30,
  };

function formatTs(ts: number | null | undefined): string {
  if (!ts) return '—';
  return new Date(ts * 1000).toLocaleString();
}

function formatTsMs(ts: number | null | undefined): string {
  if (!ts) return '—';
  return new Date(ts).toLocaleString();
}

function groundingCitationSummary(citation?: {
  citation_id: string;
  label?: string | null;
  source_sub_id?: string | null;
}): string {
  if (!citation) return '';
  const parts = [citation.citation_id];
  if (citation.label?.trim()) {
    parts.push(citation.label.trim());
  }
  if (citation.source_sub_id?.trim()) {
    parts.push(citation.source_sub_id.trim());
  }
  return parts.join(' · ');
}

function formatJobStatus(status: string): string {
  switch (status) {
    case 'queued':
      return 'Queued';
    case 'running':
      return 'Running';
    case 'completed':
      return 'Completed';
    case 'failed':
      return 'Failed';
    case 'cancelled':
      return 'Cancelled';
    case 'never':
      return 'Never';
    default:
      return status;
  }
}

function titleCase(value: string): string {
  return value
    .split(/[_\s-]+/)
    .filter(Boolean)
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(' ');
}

function formatUptime(seconds: number): string {
  const days = Math.floor(seconds / 86400);
  const hours = Math.floor((seconds % 86400) / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  if (days > 0) return `${days}d ${hours}h`;
  if (hours > 0) return `${hours}h ${minutes}m`;
  return `${minutes}m`;
}

function formatBytes(value: number | null | undefined): string {
  if (value == null || !Number.isFinite(value)) return '—';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let size = value;
  let unitIndex = 0;
  while (size >= 1024 && unitIndex < units.length - 1) {
    size /= 1024;
    unitIndex += 1;
  }
  return `${size >= 10 || unitIndex === 0 ? size.toFixed(0) : size.toFixed(1)} ${units[unitIndex]}`;
}

function formatPercent(value: number | null | undefined): string {
  if (value == null || !Number.isFinite(value)) return '—';
  return `${value.toFixed(1)}%`;
}

function aiRecommendationStatusLabel(status: string): string {
  switch (status) {
    case 'model_missing':
      return 'model missing';
    case 'not_applicable':
      return 'n/a';
    default:
      return status.replace(/_/g, ' ');
  }
}

function diagnosticsTrendTone(lastMinute: number, lastFiveMinutes: number): string {
  if (lastMinute > 0) return 'text-[#ff8a7a]';
  if (lastFiveMinutes > 0) return 'text-[#f7c67a]';
  return 'text-[#89d7a1]';
}

function DiagnosticsTrend({
  lastMinute,
  lastFiveMinutes,
}: {
  lastMinute: number;
  lastFiveMinutes: number;
}) {
  const tone = diagnosticsTrendTone(lastMinute, lastFiveMinutes);
  const dotClass =
    lastMinute > 0
      ? 'bg-[#ff7a7a]'
      : lastFiveMinutes > 0
        ? 'bg-[#f7c67a]'
        : 'bg-[#89d7a1]';
  return (
    <span className={`inline-flex items-center gap-2 font-medium ${tone}`}>
      <span className={`h-2 w-2 rounded-full ${dotClass}`} />
      <span>
        {lastMinute}/{lastFiveMinutes}
      </span>
    </span>
  );
}

function AdminToggleButton({
  checked,
  label,
  disabled = false,
  className = '',
  onToggle,
}: {
  checked: boolean;
  label: string;
  disabled?: boolean;
  className?: string;
  onToggle: () => void;
}) {
  return (
    <button
      type="button"
      data-active={checked ? 'true' : 'false'}
      aria-pressed={checked}
      disabled={disabled}
      onClick={onToggle}
      className={`rf-room-mode-btn inline-flex items-center justify-center gap-2 px-4 py-2 text-sm ${
        checked ? 'btn-primary' : 'btn-secondary'
      } ${className}`.trim()}
    >
      {label}
    </button>
  );
}

export default function AdminPage() {
  const router = useRouter();
  const { me, loading: authLoading } = useAuth();

  const [activeTab, setActiveTab] = useState<AdminTab>('users');
  const [logFilterTab, setLogFilterTab] = useState<LogFilterTab>('all');

  const [libraries, setLibraries] = useState<Library[]>([]);
  const [libraryEdits, setLibraryEdits] = useState<Record<string, LibraryEditState>>({});
  const [musicImports, setMusicImports] = useState<Record<string, MusicImportState>>({});
  const [logJobs, setLogJobs] = useState<Job[]>([]);
  const [tmdbJobs, setTmdbJobs] = useState<Job[]>([]);
  const [activeJobs, setActiveJobs] = useState<Job[]>([]);
  const [users, setUsers] = useState<UserAccount[]>([]);
  const [userEdits, setUserEdits] = useState<Record<string, UserEditState>>({});
  const [channels, setChannels] = useState<ChannelRecord[]>([]);
  const [channelEdits, setChannelEdits] = useState<Record<string, ChannelEditState>>({});
  const [rooms, setRooms] = useState<RoomRecord[]>([]);
  const [roomEdits, setRoomEdits] = useState<Record<string, RoomEditState>>({});
  const [minecraftServers, setMinecraftServers] = useState<MinecraftServer[]>([]);
  const [selectedMinecraftServerId, setSelectedMinecraftServerId] = useState<string | null>(null);
  const [selectedMinecraftServerEvents, setSelectedMinecraftServerEvents] = useState<
    MinecraftServerEvent[]
  >([]);
  const [selectedMinecraftServerLogs, setSelectedMinecraftServerLogs] = useState<ServerLogLine[]>([]);
  const [minecraftServerEventsLoading, setMinecraftServerEventsLoading] = useState(false);
  const [minecraftServerLogsLoading, setMinecraftServerLogsLoading] = useState(false);
  const [runtimeDiagnostics, setRuntimeDiagnostics] = useState<RuntimeDiagnosticsResponse | null>(null);
  const [runtimeDiagnosticsLoading, setRuntimeDiagnosticsLoading] = useState(false);
  const [vaultAuditEvents, setVaultAuditEvents] = useState<RustyVaultAuditEventResponse[]>([]);
  const [vaultAuditLoading, setVaultAuditLoading] = useState(false);
  const [vaultAuditError, setVaultAuditError] = useState('');
  const [aiAdminState, setAiAdminState] = useState<AiAdminState | null>(null);
  const [aiAdminLoading, setAiAdminLoading] = useState(false);
  const [aiAuditEvents, setAiAuditEvents] = useState<AiAssistantAuditEvent[]>([]);
  const [aiAuditError, setAiAuditError] = useState('');
  const [aiModelDirInput, setAiModelDirInput] = useState('');
  const [savingAiModelDir, setSavingAiModelDir] = useState(false);
  const [aiRemoteBackendInput, setAiRemoteBackendInput] = useState({
    enabled: false,
    base_url: '',
    model: '',
    api_key_env: '',
    timeout_secs: 120,
    supports_prompt_cache: false,
    supports_structured_output: false,
    max_parallel_requests: 1,
    overload_fallback: false,
    route_roles: [] as string[],
  });
  const [savingAiRemoteBackend, setSavingAiRemoteBackend] = useState(false);
  const [aiModelPullUrl, setAiModelPullUrl] = useState('');
  const [aiModelPullState, setAiModelPullState] = useState<AiModelPullState | null>(null);
  const [aiDeletingModel, setAiDeletingModel] = useState<string | null>(null);
  const [aiBenchmarkModelName, setAiBenchmarkModelName] = useState('');
  const [aiBenchmarkLabel, setAiBenchmarkLabel] = useState('admin-benchmark');
  const [runningAiBenchmark, setRunningAiBenchmark] = useState(false);
  const [pendingDeleteAction, setPendingDeleteAction] = useState<PendingDeleteAction | null>(
    null,
  );
  const [pendingRoomEndAction, setPendingRoomEndAction] = useState<PendingRoomEndAction | null>(
    null,
  );

  const [newLib, setNewLib] = useState({
    name: '',
    kind: 'movies',
    path: '',
    show_images: true,
    prefer_local_artwork: true,
    fetch_online_artwork: true,
    tmdb_store_in_media_dir: false,
    tmdb_sync_on_new_media: true,
    tmdb_sync_schedule: 'manual' as 'manual' | 'hourly' | 'daily' | 'weekly' | 'monthly',
    tmdb_fetch_posters: true,
    tmdb_fetch_backdrops: true,
    tmdb_fetch_metadata: true,
    tmdb_fetch_reviews: false,
  });
  const [newUser, setNewUser] = useState({
    username: '',
    password: '',
    role: 'user' as 'admin' | 'user',
    library_ids: [] as string[],
  });
  const [newChannel, setNewChannel] = useState({
    name: '',
    kind: 'text' as 'text' | 'voice',
    is_private: false,
  });

  const [hostDirBrowserOpen, setHostDirBrowserOpen] = useState(false);
  const [hostDirBrowserLoading, setHostDirBrowserLoading] = useState(false);
  const [hostDirBrowserError, setHostDirBrowserError] = useState('');
  const [hostDirBrowserCurrentPath, setHostDirBrowserCurrentPath] = useState('');
  const [hostDirBrowserParentPath, setHostDirBrowserParentPath] = useState<string | null>(null);
  const [hostDirBrowserRoots, setHostDirBrowserRoots] = useState<string[]>([]);
  const [hostDirBrowserDirectories, setHostDirBrowserDirectories] = useState<HostDirectoryListEntry[]>(
    [],
  );
  const [hostDirBrowserTargetLibraryId, setHostDirBrowserTargetLibraryId] = useState<
    string | null
  >(null);
  const [hostDirBrowserTargetAiModelDir, setHostDirBrowserTargetAiModelDir] = useState(false);
  const [tmdbConfig, setTmdbConfig] = useState<TmdbConfig>({
    configured: false,
    key_preview: null,
    source: null,
  });
  const [tmdbApiKey, setTmdbApiKey] = useState('');
  const [savingTmdb, setSavingTmdb] = useState(false);
  const [msg, setMsg] = useState('');
  const [msgType, setMsgType] = useState<'ok' | 'error'>('ok');

  const librariesRef = useRef<Library[]>([]);
  const usersRef = useRef<UserAccount[]>([]);
  const channelsRef = useRef<ChannelRecord[]>([]);
  const roomsRef = useRef<RoomRecord[]>([]);
  const aiModelPullStopRef = useRef<(() => void) | null>(null);

  function sameLibraryIds(a: string[], b: string[]): boolean {
    if (a.length !== b.length) return false;
    const setA = new Set(a);
    if (setA.size !== b.length) return false;
    return b.every((id) => setA.has(id));
  }

  function toLibraryEditState(library: Library): LibraryEditState {
    return {
      name: library.name,
      path: library.paths[0]?.path || '',
      show_images: library.settings?.show_images ?? true,
      prefer_local_artwork: library.settings?.prefer_local_artwork ?? true,
      fetch_online_artwork: library.settings?.fetch_online_artwork ?? true,
      tmdb_store_in_media_dir: library.settings?.tmdb_store_in_media_dir ?? false,
      tmdb_sync_on_new_media: library.settings?.tmdb_sync_on_new_media ?? true,
      tmdb_sync_schedule: library.settings?.tmdb_sync_schedule ?? 'manual',
      tmdb_fetch_posters: library.settings?.tmdb_fetch_posters ?? true,
      tmdb_fetch_backdrops: library.settings?.tmdb_fetch_backdrops ?? true,
      tmdb_fetch_metadata: library.settings?.tmdb_fetch_metadata ?? true,
      tmdb_fetch_reviews: library.settings?.tmdb_fetch_reviews ?? false,
    };
  }

  function sameLibraryEdit(a: LibraryEditState, b: LibraryEditState): boolean {
    return (
      a.name === b.name &&
      a.path === b.path &&
      a.show_images === b.show_images &&
      a.prefer_local_artwork === b.prefer_local_artwork &&
      a.fetch_online_artwork === b.fetch_online_artwork &&
      a.tmdb_store_in_media_dir === b.tmdb_store_in_media_dir &&
      a.tmdb_sync_on_new_media === b.tmdb_sync_on_new_media &&
      a.tmdb_sync_schedule === b.tmdb_sync_schedule &&
      a.tmdb_fetch_posters === b.tmdb_fetch_posters &&
      a.tmdb_fetch_backdrops === b.tmdb_fetch_backdrops &&
      a.tmdb_fetch_metadata === b.tmdb_fetch_metadata &&
      a.tmdb_fetch_reviews === b.tmdb_fetch_reviews
    );
  }

  function toChannelEditState(channel: ChannelRecord): ChannelEditState {
    return {
      name: channel.name,
      is_private: channel.is_private,
    };
  }

  function sameChannelEdit(a: ChannelEditState, b: ChannelEditState): boolean {
    return a.name === b.name && a.is_private === b.is_private;
  }

  function toRoomEditState(room: RoomRecord): RoomEditState {
    return {
      room_name: room.room_name,
    };
  }

  function sameRoomEdit(a: RoomEditState, b: RoomEditState): boolean {
    return a.room_name === b.room_name;
  }

  useEffect(() => {
    if (!authLoading && (!me || me.role !== 'admin')) {
      router.replace('/libraries');
    }
  }, [authLoading, me, router]);

  const loadData = useCallback(async () => {
    const logJobParams = new URLSearchParams();
    logJobParams.set('status', logFilterTab);
    logJobParams.set('limit', '300');

    const tmdbJobParams = new URLSearchParams();
    tmdbJobParams.set('kind', 'library_tmdb_sync');
    tmdbJobParams.set('limit', '1000');

    const activeJobParams = new URLSearchParams();
    activeJobParams.set('status', 'in_progress');
    activeJobParams.set('limit', '100');

    try {
      setRuntimeDiagnosticsLoading(true);
      const [
        libs,
        logJobList,
        tmdbJobList,
        activeJobList,
        userList,
        tmdb,
        channelList,
        roomList,
        minecraftServerList,
        diagnostics,
      ] = await Promise.all([
        apiJson<Library[]>('/libraries'),
        apiJson<Job[]>(`/jobs?${logJobParams.toString()}`),
        apiJson<Job[]>(`/jobs?${tmdbJobParams.toString()}`),
        apiJson<Job[]>(`/jobs?${activeJobParams.toString()}`),
        apiJson<UserAccount[]>('/users'),
        apiJson<TmdbConfig>('/system/tmdb'),
        apiJson<ChannelRecord[]>('/channels'),
        apiJson<RoomRecord[]>('/watch-party/admin/rooms'),
        listMinecraftServers(),
        apiJson<RuntimeDiagnosticsResponse>('/system/runtime-diagnostics'),
      ]);

      setLibraries(libs);
      setLibraryEdits((prev) => {
        const currentLibrariesById = new Map(
          librariesRef.current.map((library) => [library.id, library]),
        );
        const nextLibEdits: Record<string, LibraryEditState> = {};
        for (const lib of libs) {
          const serverEdit = toLibraryEditState(lib);
          const prevEdit = prev[lib.id];
          const currentLibrary = currentLibrariesById.get(lib.id);
          const hasUnsavedChanges =
            !!prevEdit &&
            !!currentLibrary &&
            !sameLibraryEdit(prevEdit, toLibraryEditState(currentLibrary));
          nextLibEdits[lib.id] = hasUnsavedChanges ? { ...prevEdit } : serverEdit;
        }
        return nextLibEdits;
      });

      setLogJobs(logJobList);
      setTmdbJobs(tmdbJobList);
      setActiveJobs(activeJobList);
      setUsers(userList);
      setUserEdits((prev) => {
        const currentUsersById = new Map(usersRef.current.map((user) => [user.id, user]));
        const nextEdits: Record<string, UserEditState> = {};
        for (const user of userList) {
          const serverEdit: UserEditState = {
            role: user.role,
            library_ids: [...(user.library_ids || [])],
          };
          const prevEdit = prev[user.id];
          const currentUser = currentUsersById.get(user.id);
          const hasUnsavedChanges =
            !!prevEdit &&
            !!currentUser &&
            (prevEdit.role !== currentUser.role ||
              !sameLibraryIds(prevEdit.library_ids, currentUser.library_ids || []));
          nextEdits[user.id] = hasUnsavedChanges
            ? {
                role: prevEdit.role,
                library_ids: [...prevEdit.library_ids],
              }
            : serverEdit;
        }
        return nextEdits;
      });

      setChannels(channelList);
      setChannelEdits((prev) => {
        const currentChannelsById = new Map(channelsRef.current.map((ch) => [ch.id, ch]));
        const nextEdits: Record<string, ChannelEditState> = {};
        for (const channel of channelList) {
          const serverEdit = toChannelEditState(channel);
          const prevEdit = prev[channel.id];
          const currentChannel = currentChannelsById.get(channel.id);
          const hasUnsavedChanges =
            !!prevEdit &&
            !!currentChannel &&
            !sameChannelEdit(prevEdit, toChannelEditState(currentChannel));
          nextEdits[channel.id] = hasUnsavedChanges ? { ...prevEdit } : serverEdit;
        }
        return nextEdits;
      });

      setRooms(roomList);
      setRoomEdits((prev) => {
        const currentRoomsById = new Map(roomsRef.current.map((room) => [room.room_id, room]));
        const nextEdits: Record<string, RoomEditState> = {};
        for (const room of roomList) {
          const serverEdit = toRoomEditState(room);
          const prevEdit = prev[room.room_id];
          const currentRoom = currentRoomsById.get(room.room_id);
          const hasUnsavedChanges =
            !!prevEdit && !!currentRoom && !sameRoomEdit(prevEdit, toRoomEditState(currentRoom));
          nextEdits[room.room_id] = hasUnsavedChanges ? { ...prevEdit } : serverEdit;
        }
        return nextEdits;
      });
      setMinecraftServers(minecraftServerList);
      setSelectedMinecraftServerId((current) => {
        if (current && minecraftServerList.some((server) => server.id === current)) {
          return current;
        }
        return minecraftServerList[0]?.id ?? null;
      });

      setTmdbConfig({
        configured: tmdb.configured,
        key_preview: tmdb.key_preview ?? null,
        source: tmdb.source ?? null,
      });
      setRuntimeDiagnostics(diagnostics);
    } catch (err: unknown) {
      setMsgType('error');
      setMsg(clientErrorMessage(err, 'Failed to load admin data'));
    } finally {
      setRuntimeDiagnosticsLoading(false);
    }
  }, [logFilterTab]);

  const loadVaultAudit = useCallback(async () => {
    try {
      setVaultAuditLoading(true);
      setVaultAuditError('');
      const session = await ensureRustyVaultWebSession();
      const response = await listRustyVaultAuditEvents(session.access_token);
      setVaultAuditEvents(response.events);
    } catch (err: unknown) {
      setVaultAuditEvents([]);
      setVaultAuditError(clientErrorMessage(err, 'Failed to load Vault audit history'));
    } finally {
      setVaultAuditLoading(false);
    }
  }, []);

  const loadAiAdmin = useCallback(async () => {
    try {
      setAiAdminLoading(true);
      setAiAuditError('');
      const [stateResult, auditResult] = await Promise.allSettled([
        fetchAiAdminState(),
        fetchAiAuditEvents(40),
      ]);

      if (stateResult.status === 'fulfilled') {
        setAiAdminState(stateResult.value);
        setAiModelDirInput(stateResult.value.model_dir);
        setAiRemoteBackendInput({
          enabled: stateResult.value.remote_backend?.enabled ?? false,
          base_url: stateResult.value.remote_backend?.base_url ?? '',
          model: stateResult.value.remote_backend?.model ?? '',
          api_key_env: stateResult.value.remote_backend?.api_key_env ?? '',
          timeout_secs: stateResult.value.remote_backend?.timeout_secs ?? 120,
          supports_prompt_cache: stateResult.value.remote_backend?.supports_prompt_cache ?? false,
          supports_structured_output:
            stateResult.value.remote_backend?.supports_structured_output ?? false,
          max_parallel_requests: stateResult.value.remote_backend?.max_parallel_requests ?? 1,
          overload_fallback: stateResult.value.remote_backend?.overload_fallback ?? false,
          route_roles: stateResult.value.remote_backend?.route_roles ?? [],
        });
        setAiBenchmarkModelName(stateResult.value.models[0]?.name ?? '');
      } else {
        setAiAdminState(null);
        throw stateResult.reason;
      }

      if (auditResult.status === 'fulfilled') {
        setAiAuditEvents(auditResult.value);
      } else {
        setAiAuditEvents([]);
        setAiAuditError(clientErrorMessage(auditResult.reason, 'Failed to load AI assistant audit'));
      }
    } catch (err: unknown) {
      setAiAdminState(null);
      setAiAuditEvents([]);
      setErr(clientErrorMessage(err, 'Failed to load AI admin state'));
    } finally {
      setAiAdminLoading(false);
    }
  }, []);

  useEffect(() => {
    if (!aiAdminState || aiAdminState.models.length === 0) {
      return;
    }
    const selected = aiBenchmarkModelName.trim();
    if (selected && aiAdminState.models.some((model) => model.name === selected)) {
      return;
    }
    setAiBenchmarkModelName(aiAdminState.models[0].name);
  }, [aiAdminState, aiBenchmarkModelName]);

  useEffect(() => {
    if (me?.role === 'admin') {
      void loadData();
    }
  }, [me, loadData]);

  useEffect(() => {
    if (me?.role !== 'admin' || activeTab !== 'vault_audit') {
      return;
    }
    void loadVaultAudit();
  }, [activeTab, loadVaultAudit, me]);

  useEffect(() => {
    if (me?.role !== 'admin' || activeTab !== 'ai') {
      return;
    }
    void loadAiAdmin();
  }, [activeTab, loadAiAdmin, me]);

  useEffect(() => {
    if (me?.role !== 'admin' || activeTab !== 'server_logs' || !selectedMinecraftServerId) {
      return;
    }

    let cancelled = false;

    const loadServerDiagnostics = async () => {
      setMinecraftServerEventsLoading(true);
      setMinecraftServerLogsLoading(true);
      try {
        const [events, logs] = await Promise.all([
          listMinecraftServerEvents(selectedMinecraftServerId, 30),
          listMinecraftServerLogs(selectedMinecraftServerId, 120),
        ]);
        if (cancelled) return;
        setSelectedMinecraftServerEvents(events);
        setSelectedMinecraftServerLogs(logs.lines);
      } catch (err: unknown) {
        if (cancelled) return;
        setSelectedMinecraftServerEvents([]);
        setSelectedMinecraftServerLogs([]);
        setErr(clientErrorMessage(err, 'Failed to load Minecraft server diagnostics'));
      } finally {
        if (!cancelled) {
          setMinecraftServerEventsLoading(false);
          setMinecraftServerLogsLoading(false);
        }
      }
    };

    void loadServerDiagnostics();
    const timer = setInterval(() => {
      void loadServerDiagnostics();
    }, 5000);

    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [activeTab, me, selectedMinecraftServerId]);

  useEffect(() => {
    librariesRef.current = libraries;
  }, [libraries]);

  useEffect(() => {
    usersRef.current = users;
  }, [users]);

  useEffect(() => {
    channelsRef.current = channels;
  }, [channels]);

  useEffect(() => {
    roomsRef.current = rooms;
  }, [rooms]);

  const hasActiveJobs = useMemo(
    () => activeJobs.length > 0,
    [activeJobs],
  );

  useEffect(() => {
    if (me?.role !== 'admin') return;
    const intervalMs = hasActiveJobs ? 1000 : 5000;
    const timer = setInterval(() => {
      void loadData();
    }, intervalMs);
    return () => clearInterval(timer);
  }, [me, hasActiveJobs, loadData]);

  const usersById = useMemo(() => {
    return new Map(users.map((u) => [u.id, u]));
  }, [users]);

  const filteredLogJobs = useMemo(() => {
    return logJobs;
  }, [logJobs]);

  const tmdbSyncStatusRows = useMemo<TmdbSyncStatusRow[]>(() => {
    const nowTs = Math.floor(Date.now() / 1000);
    const jobsByLibraryId = new Map<string, Job[]>();

    for (const job of tmdbJobs) {
      if (job.kind !== 'library_tmdb_sync') continue;
      const payload = job.payload;
      if (!payload || typeof payload !== 'object' || Array.isArray(payload)) continue;
      const libraryId = payload.library_id;
      if (typeof libraryId !== 'string' || !libraryId.trim()) continue;
      const list = jobsByLibraryId.get(libraryId) ?? [];
      list.push(job);
      jobsByLibraryId.set(libraryId, list);
    }

    for (const list of jobsByLibraryId.values()) {
      list.sort((a, b) => b.updated_ts - a.updated_ts);
    }

    return libraries
      .filter((lib) => lib.kind === 'movies' || lib.kind === 'tv_shows')
      .map((lib) => {
        const schedule = lib.settings.tmdb_sync_schedule;
        const libraryJobs = jobsByLibraryId.get(lib.id) ?? [];

        const latestActive = libraryJobs.find(
          (job) => job.status === 'queued' || job.status === 'running',
        );
        const latestTerminal = libraryJobs.find(
          (job) => job.status !== 'queued' && job.status !== 'running',
        );
        const latestFailure = libraryJobs.find(
          (job) => job.status === 'failed' && typeof job.error === 'string' && job.error.trim(),
        );

        const lastRunResult = latestTerminal?.status ?? latestActive?.status ?? 'never';
        const lastRunTs =
          latestTerminal?.updated_ts ??
          lib.settings.tmdb_last_sync_ts ??
          latestActive?.updated_ts ??
          null;

        let nextScheduledRunTs: number | null = null;
        let nextScheduledRunLabel = 'Manual only';

        if (schedule !== 'manual') {
          const intervalSeconds =
            TMDB_SCHEDULE_INTERVAL_SECONDS[
              schedule as keyof typeof TMDB_SCHEDULE_INTERVAL_SECONDS
            ] ?? null;
          if (intervalSeconds) {
            const anchorTs = lib.settings.tmdb_last_sync_ts ?? null;
            nextScheduledRunTs = anchorTs ? anchorTs + intervalSeconds : nowTs;
            nextScheduledRunLabel =
              nextScheduledRunTs <= nowTs ? 'Due now' : formatTs(nextScheduledRunTs);
          } else {
            nextScheduledRunLabel = '—';
          }
        }

        return {
          library_id: lib.id,
          library_name: lib.name,
          library_kind: lib.kind,
          last_run_result: formatJobStatus(lastRunResult),
          last_run_ts: lastRunTs,
          next_scheduled_run_ts: nextScheduledRunTs,
          next_scheduled_run_label: nextScheduledRunLabel,
          failure_reason: latestFailure?.error?.trim() || null,
        };
      })
      .sort((a, b) => a.library_name.localeCompare(b.library_name));
  }, [tmdbJobs, libraries]);

  function setOk(_message: string) {
    // Admin success toasts are intentionally suppressed in this view.
    setMsg('');
  }

  function setErr(message: string) {
    setMsgType('error');
    setMsg(message);
  }

  async function createLibrary(e: React.FormEvent) {
    e.preventDefault();
    try {
      await apiJson('/libraries', {
        method: 'POST',
        body: JSON.stringify({
          name: newLib.name,
          kind: newLib.kind,
          paths: [newLib.path],
          settings: {
            show_images: newLib.show_images,
            prefer_local_artwork: newLib.prefer_local_artwork,
            fetch_online_artwork: newLib.fetch_online_artwork,
            tmdb_store_in_media_dir: newLib.tmdb_store_in_media_dir,
            tmdb_sync_on_new_media: newLib.tmdb_sync_on_new_media,
            tmdb_sync_schedule: newLib.tmdb_sync_schedule,
            tmdb_fetch_posters: newLib.tmdb_fetch_posters,
            tmdb_fetch_backdrops: newLib.tmdb_fetch_backdrops,
            tmdb_fetch_metadata: newLib.tmdb_fetch_metadata,
            tmdb_fetch_reviews: newLib.tmdb_fetch_reviews,
          },
        }),
      });
      setOk('Library created');
      setNewLib({
        name: '',
        kind: 'movies',
        path: '',
        show_images: true,
        prefer_local_artwork: true,
        fetch_online_artwork: true,
        tmdb_store_in_media_dir: false,
        tmdb_sync_on_new_media: true,
        tmdb_sync_schedule: 'manual',
        tmdb_fetch_posters: true,
        tmdb_fetch_backdrops: true,
        tmdb_fetch_metadata: true,
        tmdb_fetch_reviews: false,
      });
      await loadData();
    } catch (err: unknown) {
      setErr(clientErrorMessage(err, 'Failed to create library'));
    }
  }

  async function scanLibrary(libId: string) {
    try {
      await apiJson(`/libraries/${libId}/scan`, { method: 'POST' });
      setOk('Scan started');
      await loadData();
    } catch (err: unknown) {
      setErr(clientErrorMessage(err, 'Failed to start scan'));
    }
  }

  async function syncLibraryTmdb(libId: string) {
    try {
      await apiJson(`/libraries/${libId}/tmdb-sync`, { method: 'POST' });
      setOk('TMDB sync started');
      await loadData();
    } catch (err: unknown) {
      setErr(clientErrorMessage(err, 'Failed to start TMDB sync'));
    }
  }

  function getMusicImportState(libraryId: string): MusicImportState {
    return musicImports[libraryId] ?? defaultMusicImportState();
  }

  function setMusicImportField<K extends keyof MusicImportState>(
    libraryId: string,
    key: K,
    value: MusicImportState[K],
  ) {
    setMusicImports((prev) => ({
      ...prev,
      [libraryId]: {
        ...(prev[libraryId] ?? defaultMusicImportState()),
        [key]: value,
      },
    }));
  }

  async function importMusicToLibrary(libraryId: string) {
    const current = getMusicImportState(libraryId);
    const source = current.source.trim();
    const artist = current.artist.trim();
    const title = current.title.trim();
    const album = current.album.trim();

    if (!source || !artist || !title) {
      setErr('YouTube source, artist, and title are required for music import');
      return;
    }

    setMusicImportField(libraryId, 'importing', true);
    try {
      const response = await apiJson<MusicImportResponse>(
        `/libraries/${libraryId}/music/import-youtube`,
        {
          method: 'POST',
          body: JSON.stringify({
            source,
            artist,
            album,
            title,
          }),
        },
      );
      setMusicImports((prev) => ({
        ...prev,
        [libraryId]: {
          ...defaultMusicImportState(),
          importing: false,
          artist: response.artist,
          album: response.album,
          title: response.title,
        },
      }));
      setOk(
        `Imported "${response.title}" by ${response.artist} to ${response.album}. Library scan job queued.`,
      );
      await loadData();
    } catch (err: unknown) {
      setMusicImportField(libraryId, 'importing', false);
      setErr(clientErrorMessage(err, 'Failed to import YouTube audio into library'));
    }
  }

  async function fetchHostDirectories(path?: string) {
    const query = path?.trim();
    const endpoint = query
      ? `/system/host-directories?path=${encodeURIComponent(query)}`
      : '/system/host-directories';
    const data = await apiJson<HostDirectoryListResponse>(endpoint);
    setHostDirBrowserCurrentPath(data.current_path);
    setHostDirBrowserParentPath(data.parent_path);
    setHostDirBrowserRoots(data.roots);
    setHostDirBrowserDirectories(data.directories);
  }

  function closeHostDirectoryBrowser() {
    setHostDirBrowserOpen(false);
    setHostDirBrowserLoading(false);
    setHostDirBrowserError('');
    setHostDirBrowserTargetLibraryId(null);
    setHostDirBrowserTargetAiModelDir(false);
  }

  function openHostDirectoryBrowser(
    targetLibraryId: string | null,
    initialPath?: string,
    options?: { aiModelDir?: boolean },
  ) {
    setHostDirBrowserOpen(true);
    setHostDirBrowserTargetLibraryId(targetLibraryId);
    setHostDirBrowserTargetAiModelDir(Boolean(options?.aiModelDir));
    setHostDirBrowserError('');
    setHostDirBrowserLoading(true);
    void fetchHostDirectories(initialPath)
      .catch((err: unknown) => {
        setHostDirBrowserError(clientErrorMessage(err, 'Failed to browse backend directories'));
      })
      .finally(() => {
        setHostDirBrowserLoading(false);
      });
  }

  function browseLibraryPath() {
    openHostDirectoryBrowser(null, newLib.path);
  }

  function setLibraryEdit<K extends keyof LibraryEditState>(
    libraryId: string,
    key: K,
    value: LibraryEditState[K],
  ) {
    setLibraryEdits((prev) => ({
      ...prev,
      [libraryId]: {
        ...(prev[libraryId] || {
          name: '',
          path: '',
          show_images: true,
          prefer_local_artwork: true,
          fetch_online_artwork: true,
          tmdb_store_in_media_dir: false,
          tmdb_sync_on_new_media: true,
          tmdb_sync_schedule: 'manual',
          tmdb_fetch_posters: true,
          tmdb_fetch_backdrops: true,
          tmdb_fetch_metadata: true,
          tmdb_fetch_reviews: false,
        }),
        [key]: value,
      },
    }));
  }

  function browseExistingLibraryPath(libraryId: string) {
    const editPath = libraryEdits[libraryId]?.path;
    const existingPath = libraries.find((library) => library.id === libraryId)?.paths[0]?.path;
    openHostDirectoryBrowser(libraryId, editPath || existingPath || '');
  }

  function browseAiModelDirectory() {
    openHostDirectoryBrowser(null, aiModelDirInput, { aiModelDir: true });
  }

  function confirmHostDirectorySelection() {
    if (!hostDirBrowserCurrentPath.trim()) {
      setHostDirBrowserError('No directory selected');
      return;
    }
    if (hostDirBrowserTargetAiModelDir) {
      setAiModelDirInput(hostDirBrowserCurrentPath);
    } else if (hostDirBrowserTargetLibraryId) {
      setLibraryEdit(hostDirBrowserTargetLibraryId, 'path', hostDirBrowserCurrentPath);
    } else {
      setNewLib((prev) => ({ ...prev, path: hostDirBrowserCurrentPath }));
    }
    setOk('Directory selected');
    closeHostDirectoryBrowser();
  }

  function navigateHostDirectory(path?: string | null) {
    const target = path?.trim();
    if (!target) return;
    setHostDirBrowserError('');
    setHostDirBrowserLoading(true);
    void fetchHostDirectories(target)
      .catch((err: unknown) => {
        setHostDirBrowserError(clientErrorMessage(err, 'Failed to browse backend directories'));
      })
      .finally(() => {
        setHostDirBrowserLoading(false);
      });
  }

  async function saveLibrary(libraryId: string) {
    const edit = libraryEdits[libraryId];
    if (!edit) return;
    if (!edit.path.trim()) {
      setErr('Library path is required');
      return;
    }
    try {
      await apiJson(`/libraries/${libraryId}`, {
        method: 'PATCH',
        body: JSON.stringify({
          name: edit.name,
          paths: [edit.path],
          settings: {
            show_images: edit.show_images,
            prefer_local_artwork: edit.prefer_local_artwork,
            fetch_online_artwork: edit.fetch_online_artwork,
            tmdb_store_in_media_dir: edit.tmdb_store_in_media_dir,
            tmdb_sync_on_new_media: edit.tmdb_sync_on_new_media,
            tmdb_sync_schedule: edit.tmdb_sync_schedule,
            tmdb_fetch_posters: edit.tmdb_fetch_posters,
            tmdb_fetch_backdrops: edit.tmdb_fetch_backdrops,
            tmdb_fetch_metadata: edit.tmdb_fetch_metadata,
            tmdb_fetch_reviews: edit.tmdb_fetch_reviews,
          },
        }),
      });
      const nextLibraries = libraries.map((library) => {
        if (library.id !== libraryId) return library;
        const nextPath = library.paths[0]
          ? [{ ...library.paths[0], path: edit.path }, ...library.paths.slice(1)]
          : [{ id: 'primary', path: edit.path, is_read_only: false }];
        return {
          ...library,
          name: edit.name,
          paths: nextPath,
          settings: {
            ...library.settings,
            show_images: edit.show_images,
            prefer_local_artwork: edit.prefer_local_artwork,
            fetch_online_artwork: edit.fetch_online_artwork,
            tmdb_store_in_media_dir: edit.tmdb_store_in_media_dir,
            tmdb_sync_on_new_media: edit.tmdb_sync_on_new_media,
            tmdb_sync_schedule: edit.tmdb_sync_schedule,
            tmdb_fetch_posters: edit.tmdb_fetch_posters,
            tmdb_fetch_backdrops: edit.tmdb_fetch_backdrops,
            tmdb_fetch_metadata: edit.tmdb_fetch_metadata,
            tmdb_fetch_reviews: edit.tmdb_fetch_reviews,
          },
        };
      });
      librariesRef.current = nextLibraries;
      setLibraries(nextLibraries);
      setLibraryEdits((prev) => ({
        ...prev,
        [libraryId]: { ...edit },
      }));
      setOk('Library updated');
      await loadData();
    } catch (err: unknown) {
      setErr(clientErrorMessage(err, 'Failed to update library'));
    }
  }

  function requestDeleteLibrary(libId: string) {
    const target = libraries.find((l) => l.id === libId);
    const label = target ? `"${target.name}"` : 'this library';
    setPendingDeleteAction({ kind: 'library', id: libId, label });
  }

  async function deleteLibrary(libId: string) {
    try {
      const targetEl = findDataDeleteTarget('data-admin-library-card-id', libId);
      await playTelegramDeleteAnimation(targetEl);
      await apiJson<void>(`/libraries/${libId}`, { method: 'DELETE' });
      setOk('Library deleted');
      await loadData();
    } catch (err: unknown) {
      setErr(clientErrorMessage(err, 'Failed to delete library'));
    }
  }

  function toggleIds(ids: string[], id: string): string[] {
    return ids.includes(id) ? ids.filter((v) => v !== id) : [...ids, id];
  }

  async function createUser(e: React.FormEvent) {
    e.preventDefault();
    try {
      await apiJson('/users', {
        method: 'POST',
        body: JSON.stringify({
          username: newUser.username,
          password: newUser.password,
          role: newUser.role,
          library_ids: newUser.role === 'user' ? newUser.library_ids : [],
        }),
      });
      setOk('User created');
      setNewUser({
        username: '',
        password: '',
        role: 'user',
        library_ids: [],
      });
      await loadData();
    } catch (err: unknown) {
      setErr(clientErrorMessage(err, 'Failed to create user'));
    }
  }

  function updateEditRole(userId: string, role: 'admin' | 'user') {
    setUserEdits((prev) => ({
      ...prev,
      [userId]: {
        role,
        library_ids: role === 'admin' ? [] : prev[userId]?.library_ids || [],
      },
    }));
  }

  function toggleEditLibrary(userId: string, libraryId: string) {
    setUserEdits((prev) => {
      const current = prev[userId] || { role: 'user' as const, library_ids: [] };
      return {
        ...prev,
        [userId]: {
          ...current,
          library_ids: toggleIds(current.library_ids, libraryId),
        },
      };
    });
  }

  async function saveUserPermissions(userId: string) {
    const edit = userEdits[userId];
    if (!edit) return;
    try {
      await apiJson(`/users/${userId}`, {
        method: 'PATCH',
        body: JSON.stringify({
          role: edit.role,
          library_ids: edit.role === 'user' ? edit.library_ids : [],
        }),
      });
      const savedRole = edit.role;
      const savedLibraryIds = edit.role === 'user' ? [...edit.library_ids] : [];
      const nextUsers = users.map((user) =>
        user.id === userId
          ? {
              ...user,
              role: savedRole,
              library_ids: [...savedLibraryIds],
            }
          : user,
      );
      usersRef.current = nextUsers;
      setUsers(nextUsers);
      setUserEdits((prev) => ({
        ...prev,
        [userId]: {
          role: savedRole,
          library_ids: [...savedLibraryIds],
        },
      }));
      setOk('User permissions updated');
      await loadData();
    } catch (err: unknown) {
      setErr(clientErrorMessage(err, 'Failed to update permissions'));
    }
  }

  async function deleteUser(userId: string) {
    try {
      const targetEl = findDataDeleteTarget('data-admin-user-card-id', userId);
      await playTelegramDeleteAnimation(targetEl);
      await apiJson<void>(`/users/${userId}`, { method: 'DELETE' });
      setOk('User deleted');
      await loadData();
    } catch (err: unknown) {
      setErr(clientErrorMessage(err, 'Failed to delete user'));
    }
  }

  function requestDeleteUser(userId: string) {
    const target = users.find((user) => user.id === userId);
    const label = target ? `"${target.username}"` : 'this user';
    setPendingDeleteAction({ kind: 'user', id: userId, label });
  }

  async function createChannel(e: React.FormEvent) {
    e.preventDefault();
    try {
      await apiJson('/channels', {
        method: 'POST',
        body: JSON.stringify(newChannel),
      });
      setOk('Channel created');
      setNewChannel({
        name: '',
        kind: 'text',
        is_private: false,
      });
      await loadData();
    } catch (err: unknown) {
      setErr(clientErrorMessage(err, 'Failed to create channel'));
    }
  }

  function setChannelEdit<K extends keyof ChannelEditState>(
    channelId: string,
    key: K,
    value: ChannelEditState[K],
  ) {
    setChannelEdits((prev) => ({
      ...prev,
      [channelId]: {
        ...(prev[channelId] || {
          name: '',
          is_private: false,
        }),
        [key]: value,
      },
    }));
  }

  async function saveChannel(channelId: string) {
    const edit = channelEdits[channelId];
    if (!edit) return;
    if (!edit.name.trim()) {
      setErr('Channel name is required');
      return;
    }
    try {
      await apiJson(`/channels/${channelId}`, {
        method: 'PATCH',
        body: JSON.stringify({
          name: edit.name,
          is_private: edit.is_private,
        }),
      });
      const nextChannels = channels.map((channel) =>
        channel.id === channelId
          ? {
              ...channel,
              name: edit.name,
              is_private: edit.is_private,
            }
          : channel,
      );
      channelsRef.current = nextChannels;
      setChannels(nextChannels);
      setChannelEdits((prev) => ({
        ...prev,
        [channelId]: { ...edit },
      }));
      setOk('Channel updated');
      await loadData();
    } catch (err: unknown) {
      setErr(clientErrorMessage(err, 'Failed to update channel'));
    }
  }

  function requestDeleteChannel(channelId: string) {
    const target = channels.find((c) => c.id === channelId);
    const label = target ? `"${target.name}"` : 'this channel';
    setPendingDeleteAction({ kind: 'channel', id: channelId, label });
  }

  async function deleteChannel(channelId: string) {
    try {
      const targetEl = findDataDeleteTarget('data-admin-channel-card-id', channelId);
      await playTelegramDeleteAnimation(targetEl);
      await apiJson<void>(`/channels/${channelId}`, { method: 'DELETE' });
      setOk('Channel deleted');
      await loadData();
    } catch (err: unknown) {
      setErr(clientErrorMessage(err, 'Failed to delete channel'));
    }
  }

  function setRoomEdit<K extends keyof RoomEditState>(
    roomId: string,
    key: K,
    value: RoomEditState[K],
  ) {
    setRoomEdits((prev) => ({
      ...prev,
      [roomId]: {
        ...(prev[roomId] || {
          room_name: '',
        }),
        [key]: value,
      },
    }));
  }

  async function saveRoomName(roomId: string) {
    const edit = roomEdits[roomId];
    if (!edit) return;
    try {
      await apiJson(`/watch-party/admin/rooms/${roomId}/rename`, {
        method: 'PATCH',
        body: JSON.stringify({ room_name: edit.room_name }),
      });
      const nextRooms = rooms.map((room) =>
        room.room_id === roomId
          ? {
              ...room,
              room_name: edit.room_name,
            }
          : room,
      );
      roomsRef.current = nextRooms;
      setRooms(nextRooms);
      setRoomEdits((prev) => ({
        ...prev,
        [roomId]: { ...edit },
      }));
      setOk('Room renamed');
      await loadData();
    } catch (err: unknown) {
      setErr(clientErrorMessage(err, 'Failed to rename room'));
    }
  }

  async function endRoom(roomId: string) {
    setPendingRoomEndAction(null);
    const target = rooms.find((r) => r.room_id === roomId);
    const label = target?.title || roomId;
    try {
      await apiJson(`/watch-party/admin/rooms/${roomId}/end`, {
        method: 'POST',
      });
      setOk(`Room "${label}" ended`);
      await loadData();
    } catch (err: unknown) {
      setErr(clientErrorMessage(err, 'Failed to end room'));
    }
  }

  function requestEndRoom(roomId: string) {
    const target = rooms.find((r) => r.room_id === roomId);
    const label = target?.title || roomId;
    setPendingRoomEndAction({ id: roomId, label: `"${label}"` });
  }

  async function deleteRoom(roomId: string) {
    try {
      const targetEl = findDataDeleteTarget('data-admin-room-card-id', roomId);
      await playTelegramDeleteAnimation(targetEl);
      await apiJson<void>(`/watch-party/admin/rooms/${roomId}`, {
        method: 'DELETE',
      });
      setOk('Room deleted');
      await loadData();
    } catch (err: unknown) {
      setErr(clientErrorMessage(err, 'Failed to delete room'));
    }
  }

  function requestDeleteRoom(roomId: string) {
    const target = rooms.find((room) => room.room_id === roomId);
    const label = target ? `"${target.title}"` : 'this room';
    setPendingDeleteAction({ kind: 'room', id: roomId, label });
  }

  async function confirmPendingDelete() {
    if (!pendingDeleteAction) return;
    const pending = pendingDeleteAction;
    setPendingDeleteAction(null);
    if (pending.kind === 'user') {
      await deleteUser(pending.id);
      return;
    }
    if (pending.kind === 'library') {
      await deleteLibrary(pending.id);
      return;
    }
    if (pending.kind === 'channel') {
      await deleteChannel(pending.id);
      return;
    }
    await deleteRoom(pending.id);
  }

  async function saveTmdbKey(e: React.FormEvent) {
    e.preventDefault();
    setSavingTmdb(true);
    try {
      const updated = await apiJson<TmdbConfig>('/system/tmdb', {
        method: 'PUT',
        body: JSON.stringify({ api_key: tmdbApiKey }),
      });
      setTmdbConfig({
        configured: updated.configured,
        key_preview: updated.key_preview ?? null,
        source: updated.source ?? null,
      });
      setTmdbApiKey('');
      setOk(updated.configured ? 'TMDB key saved' : 'TMDB key cleared');
    } catch (err: unknown) {
      setErr(clientErrorMessage(err, 'Failed to save TMDB key'));
    } finally {
      setSavingTmdb(false);
    }
  }

  async function clearTmdbKey() {
    setSavingTmdb(true);
    try {
      const updated = await apiJson<TmdbConfig>('/system/tmdb', {
        method: 'PUT',
        body: JSON.stringify({ api_key: '' }),
      });
      setTmdbConfig({
        configured: updated.configured,
        key_preview: updated.key_preview ?? null,
        source: updated.source ?? null,
      });
      setTmdbApiKey('');
      setOk(updated.configured ? 'Using environment TMDB key' : 'TMDB key cleared');
    } catch (err: unknown) {
      setErr(clientErrorMessage(err, 'Failed to clear TMDB key'));
    } finally {
      setSavingTmdb(false);
    }
  }

  async function saveAiModelDirectory() {
    setSavingAiModelDir(true);
    try {
      const updated = await updateAiModelDir(aiModelDirInput.trim());
      setAiAdminState(updated);
      setAiModelDirInput(updated.model_dir);
      setOk('AI model directory updated');
    } catch (err: unknown) {
      setErr(clientErrorMessage(err, 'Failed to save AI model directory'));
    } finally {
      setSavingAiModelDir(false);
    }
  }

  async function resetAiModelDirectory() {
    setSavingAiModelDir(true);
    try {
      const updated = await updateAiModelDir('');
      setAiAdminState(updated);
      setAiModelDirInput(updated.model_dir);
      setOk('AI model directory reset');
    } catch (err: unknown) {
      setErr(clientErrorMessage(err, 'Failed to reset AI model directory'));
    } finally {
      setSavingAiModelDir(false);
    }
  }

  async function saveAiRemoteBackend() {
    setSavingAiRemoteBackend(true);
    try {
      const updated = await updateAiRemoteBackend({
        enabled: aiRemoteBackendInput.enabled,
        base_url: aiRemoteBackendInput.base_url,
        model: aiRemoteBackendInput.model,
        api_key_env: aiRemoteBackendInput.api_key_env.trim() || null,
        timeout_secs: aiRemoteBackendInput.timeout_secs,
        supports_prompt_cache: aiRemoteBackendInput.supports_prompt_cache,
        supports_structured_output: aiRemoteBackendInput.supports_structured_output,
        max_parallel_requests: aiRemoteBackendInput.max_parallel_requests,
        overload_fallback: aiRemoteBackendInput.overload_fallback,
        route_roles: aiRemoteBackendInput.route_roles,
      });
      setAiAdminState(updated);
      setAiModelDirInput(updated.model_dir);
      setAiRemoteBackendInput({
        enabled: updated.remote_backend?.enabled ?? false,
        base_url: updated.remote_backend?.base_url ?? '',
        model: updated.remote_backend?.model ?? '',
        api_key_env: updated.remote_backend?.api_key_env ?? '',
        timeout_secs: updated.remote_backend?.timeout_secs ?? 120,
        supports_prompt_cache: updated.remote_backend?.supports_prompt_cache ?? false,
        supports_structured_output: updated.remote_backend?.supports_structured_output ?? false,
        max_parallel_requests: updated.remote_backend?.max_parallel_requests ?? 1,
        overload_fallback: updated.remote_backend?.overload_fallback ?? false,
        route_roles: updated.remote_backend?.route_roles ?? [],
      });
      setOk('AI remote backend updated');
    } catch (err: unknown) {
      setErr(clientErrorMessage(err, 'Failed to save AI remote backend'));
    } finally {
      setSavingAiRemoteBackend(false);
    }
  }

  async function runAiBenchmark() {
    if (!aiBenchmarkModelName.trim()) {
      setErr('Choose a model before running the benchmark.');
      return;
    }

    setRunningAiBenchmark(true);
    try {
      const updated = await runAiModelBenchmark({
        model_name: aiBenchmarkModelName.trim(),
        benchmark_label: aiBenchmarkLabel.trim() || null,
      });
      setAiAdminState(updated);
      setAiModelDirInput(updated.model_dir);
      setAiRemoteBackendInput({
        enabled: updated.remote_backend?.enabled ?? false,
        base_url: updated.remote_backend?.base_url ?? '',
        model: updated.remote_backend?.model ?? '',
        api_key_env: updated.remote_backend?.api_key_env ?? '',
        timeout_secs: updated.remote_backend?.timeout_secs ?? 120,
        supports_prompt_cache: updated.remote_backend?.supports_prompt_cache ?? false,
        supports_structured_output: updated.remote_backend?.supports_structured_output ?? false,
        max_parallel_requests: updated.remote_backend?.max_parallel_requests ?? 1,
        overload_fallback: updated.remote_backend?.overload_fallback ?? false,
        route_roles: updated.remote_backend?.route_roles ?? [],
      });
      setOk(`AI benchmark completed for ${aiBenchmarkModelName.trim()}`);
    } catch (err: unknown) {
      setErr(clientErrorMessage(err, 'Failed to run AI benchmark'));
    } finally {
      setRunningAiBenchmark(false);
    }
  }

  function pullAiModel() {
    const url = aiModelPullUrl.trim();
    if (!url || aiModelPullState?.active) return;

    setAiModelPullState({
      status: 'Starting…',
      percent: 0,
      active: true,
      done: false,
      error: null,
    });

    aiModelPullStopRef.current = pullAiModelFromUrl(
      url,
      (event: AdminAiPullEvent) => {
        if (event.type === 'progress') {
          setAiModelPullState({
            status: event.status,
            percent: event.percent,
            active: true,
            done: false,
            error: null,
          });
          return;
        }
        if (event.type === 'done') {
          setAiModelPullState({
            status: 'Complete',
            percent: 100,
            active: false,
            done: true,
            error: null,
          });
          setAiModelPullUrl('');
          void loadAiAdmin();
          return;
        }
        setAiModelPullState({
          status: 'Failed',
          percent: 0,
          active: false,
          done: false,
          error: event.message,
        });
      },
      () => {
        setAiModelPullState((current) =>
          current?.active
            ? {
                ...current,
                active: false,
                status: 'Cancelled',
              }
            : current,
        );
      },
    );
  }

  function cancelAiModelPull() {
    if (aiModelPullStopRef.current) {
      aiModelPullStopRef.current();
    }
  }

  async function removeAiModel(name: string) {
    if (aiDeletingModel) return;
    setAiDeletingModel(name);
    try {
      await deleteAiModel(name);
      await loadAiAdmin();
      setOk('AI model removed');
    } catch (err: unknown) {
      setErr(clientErrorMessage(err, 'Failed to remove AI model'));
    } finally {
      setAiDeletingModel(null);
    }
  }

  async function deleteJob(jobId: string) {
    try {
      const targetEl = findDataDeleteTarget('data-admin-job-id', jobId);
      await playTelegramDeleteAnimation(targetEl);
      await apiJson<void>(`/jobs/${jobId}`, { method: 'DELETE' });
      setOk('Log entry deleted');
      await loadData();
    } catch (err: unknown) {
      setErr(clientErrorMessage(err, 'Failed to delete log entry'));
    }
  }

  if (authLoading) {
    return (
      <div className="rf-flat-empty px-5 py-4">
        <p className="text-sm muted">Checking access…</p>
      </div>
    );
  }

  if (!me || me.role !== 'admin') {
    return (
      <div className="rf-flat-empty px-6 py-8">
        <p className="text-sm muted">Admin access required.</p>
      </div>
    );
  }

  const adminUsers = users.filter((u) => u.role === 'admin');
  const regularUsers = users.filter((u) => u.role !== 'admin');
  const selectedMinecraftServer =
    minecraftServers.find((server) => server.id === selectedMinecraftServerId) ?? null;

  return (
    <div className="rf-flat-page rf-flat-scope animate-rise">
      <header className="rf-flat-header">
        <h1 className="text-3xl font-semibold sm:text-4xl">Admin Dashboard</h1>
      </header>

      {msg && (
        <p className={`${msgType === 'ok' ? 'notice-ok' : 'notice-error'} rounded-xl px-4 py-2 text-sm`}>
          {msg}
        </p>
      )}

      <div className="rf-top-tabbar border-b border-[var(--border)]/70 pb-0">
        {ADMIN_TABS.map((tab) => (
          <button
            key={tab.key}
            type="button"
            onClick={() => setActiveTab(tab.key)}
            className="rf-top-tab"
            data-active={activeTab === tab.key}
          >
            {tab.label}
          </button>
        ))}
      </div>

      {activeTab === 'users' && (
        <div className="space-y-8">
          <section className="panel space-y-4 p-6">
            <h2 className="text-xl font-semibold">Create User</h2>
            <form onSubmit={createUser} className="space-y-4">
              <div className="grid grid-cols-1 gap-3 md:grid-cols-3">
                <input
                  placeholder="Username"
                  value={newUser.username}
                  onChange={(e) => setNewUser({ ...newUser, username: e.target.value })}
                  className="input px-3 py-2 text-sm"
                  required
                />
                <input
                  type="password"
                  placeholder="Password (min 6 chars)"
                  minLength={6}
                  value={newUser.password}
                  onChange={(e) => setNewUser({ ...newUser, password: e.target.value })}
                  className="input px-3 py-2 text-sm"
                  required
                />
                <select
                  aria-label="New user role"
                  value={newUser.role}
                  onChange={(e) =>
                    setNewUser({
                      ...newUser,
                      role: e.target.value as 'admin' | 'user',
                      library_ids: e.target.value === 'admin' ? [] : newUser.library_ids,
                    })
                  }
                  className="select px-3 py-2 text-sm"
                >
                  <option value="user">User</option>
                  <option value="admin">Admin</option>
                </select>
              </div>

              {newUser.role === 'user' && (
                <div className="space-y-2">
                  <p className="text-sm font-medium">Allowed Libraries</p>
                  {libraries.length === 0 ? (
                    <p className="text-xs muted">
                      No libraries configured yet. You can create this user now and assign access later.
                    </p>
                  ) : (
                    <div className="grid grid-cols-1 gap-2 md:grid-cols-2">
                      {libraries.map((lib) => (
                        <AdminToggleButton
                          key={lib.id}
                          checked={newUser.library_ids.includes(lib.id)}
                          label={lib.name}
                          className="w-full justify-start"
                          onToggle={() =>
                            setNewUser({
                              ...newUser,
                              library_ids: toggleIds(newUser.library_ids, lib.id),
                            })
                          }
                        />
                      ))}
                    </div>
                  )}
                </div>
              )}

              <button type="submit" className="btn-primary px-4 py-2 text-sm">
                Create User
              </button>
            </form>
          </section>

          <section className="space-y-4">
            <h2 className="text-xl font-semibold">Manage Users</h2>
            {users.length === 0 ? (
              <div className="panel-soft px-4 py-3">
                <p className="text-sm muted">No users found.</p>
              </div>
            ) : (
              <div className="space-y-4">
                {adminUsers.length > 0 && (
                  <div className="space-y-3">
                    <p className="text-sm font-medium muted">Admin Accounts</p>
                    <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-3">
                      {adminUsers.map((user) => {
                        const edit = userEdits[user.id] || {
                          role: user.role,
                          library_ids: user.library_ids || [],
                        };
                        return (
                          <div key={user.id} data-admin-user-card-id={user.id} className="tile space-y-3 p-4">
                            <div>
                              <p className="font-medium">{user.username}</p>
                              <p className="text-xs muted">
                                {new Date(user.created_ts * 1000).toLocaleString()}
                              </p>
                            </div>
                            <select
                              aria-label={`Role for ${user.username}`}
                              value={edit.role}
                              onChange={(e) => updateEditRole(user.id, e.target.value as 'admin' | 'user')}
                              className="select w-full px-2 py-1.5 text-sm"
                            >
                              <option value="user">User</option>
                              <option value="admin">Admin</option>
                            </select>
                            <div className="flex gap-2 pt-1">
                              <button
                                onClick={() => saveUserPermissions(user.id)}
                                className="btn-primary flex-1 px-3 py-1.5 text-sm"
                              >
                                Save
                              </button>
                              <button
                                onClick={() => requestDeleteUser(user.id)}
                                className="btn-ghost px-3 py-1.5 text-sm text-[var(--danger)]"
                              >
                                Delete
                              </button>
                            </div>
                          </div>
                        );
                      })}
                    </div>
                  </div>
                )}

                {adminUsers.length > 0 && regularUsers.length > 0 && (
                  <div className="border-t border-[var(--border)]" />
                )}

                {regularUsers.length > 0 && (
                  <div className="space-y-3">
                    <p className="text-sm font-medium muted">User Accounts</p>
                    <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-3">
                      {regularUsers.map((user) => {
                        const edit = userEdits[user.id] || {
                          role: user.role,
                          library_ids: user.library_ids || [],
                        };
                        return (
                          <div key={user.id} data-admin-user-card-id={user.id} className="tile space-y-3 p-4">
                            <div>
                              <p className="font-medium">{user.username}</p>
                              <p className="text-xs muted">
                                {new Date(user.created_ts * 1000).toLocaleString()}
                              </p>
                            </div>
                            <select
                              aria-label={`Role for ${user.username}`}
                              value={edit.role}
                              onChange={(e) => updateEditRole(user.id, e.target.value as 'admin' | 'user')}
                              className="select w-full px-2 py-1.5 text-sm"
                            >
                              <option value="user">User</option>
                              <option value="admin">Admin</option>
                            </select>
                            {edit.role === 'user' && libraries.length > 0 && (
                              <div className="space-y-1.5">
                                <p className="text-xs uppercase tracking-[0.18em] muted">Libraries</p>
                                <div className="space-y-1">
                                  {libraries.map((lib) => (
                                    <AdminToggleButton
                                      key={lib.id}
                                      checked={edit.library_ids.includes(lib.id)}
                                      label={lib.name}
                                      className="w-full justify-start"
                                      onToggle={() => toggleEditLibrary(user.id, lib.id)}
                                    />
                                  ))}
                                </div>
                              </div>
                            )}
                            <div className="flex gap-2 pt-1">
                              <button
                                onClick={() => saveUserPermissions(user.id)}
                                className="btn-primary flex-1 px-3 py-1.5 text-sm"
                              >
                                Save
                              </button>
                              <button
                                onClick={() => requestDeleteUser(user.id)}
                                className="btn-ghost px-3 py-1.5 text-sm text-[var(--danger)]"
                              >
                                Delete
                              </button>
                            </div>
                          </div>
                        );
                      })}
                    </div>
                  </div>
                )}
              </div>
            )}
          </section>
        </div>
      )}

      {activeTab === 'libraries' && (
        <div className="space-y-8">
          <section className="panel space-y-4 p-6">
            <h2 className="text-xl font-semibold">Create Library</h2>
            <form onSubmit={createLibrary} className="space-y-4">
              <div className="grid grid-cols-1 gap-3 md:grid-cols-[1.1fr_0.9fr_2fr_auto_auto]">
                <input
                  placeholder="Name"
                  value={newLib.name}
                  onChange={(e) => setNewLib({ ...newLib, name: e.target.value })}
                  className="input px-3 py-2 text-sm"
                  required
                />
                <select
                  aria-label="Library type"
                  value={newLib.kind}
                  onChange={(e) => setNewLib({ ...newLib, kind: e.target.value })}
                  className="select px-3 py-2 text-sm"
                >
                  <option value="movies">Movies</option>
                  <option value="tv_shows">TV Shows</option>
                  <option value="music">Music</option>
                </select>
                <input
                  placeholder="/path/to/media"
                  value={newLib.path}
                  onChange={(e) => setNewLib({ ...newLib, path: e.target.value })}
                  className="input px-3 py-2 text-sm"
                  required
                />
                <button
                  type="button"
                  onClick={browseLibraryPath}
                  disabled={
                    hostDirBrowserOpen &&
                    hostDirBrowserLoading &&
                    hostDirBrowserTargetLibraryId === null
                  }
                  className="btn-secondary px-4 py-2 text-sm disabled:opacity-50"
                >
                  {hostDirBrowserOpen &&
                  hostDirBrowserLoading &&
                  hostDirBrowserTargetLibraryId === null
                    ? 'Loading...'
                    : 'Browse Host'}
                </button>
                <button type="submit" className="btn-primary px-4 py-2 text-sm">
                  Create
                </button>
              </div>

              <div className="grid grid-cols-1 gap-2 md:grid-cols-2 xl:grid-cols-3">
                <AdminToggleButton
                  checked={newLib.show_images}
                  label="Enable artwork thumbnails"
                  className="w-full justify-start"
                  onToggle={() => setNewLib({ ...newLib, show_images: !newLib.show_images })}
                />
                <AdminToggleButton
                  checked={newLib.prefer_local_artwork}
                  label="Prefer local artwork files"
                  className="w-full justify-start"
                  onToggle={() =>
                    setNewLib({ ...newLib, prefer_local_artwork: !newLib.prefer_local_artwork })
                  }
                />
                <AdminToggleButton
                  checked={newLib.fetch_online_artwork}
                  label="Fetch missing artwork online"
                  className="w-full justify-start"
                  onToggle={() =>
                    setNewLib({ ...newLib, fetch_online_artwork: !newLib.fetch_online_artwork })
                  }
                />
                <AdminToggleButton
                  checked={newLib.tmdb_store_in_media_dir}
                  label="Store TMDB images in media folders"
                  className="w-full justify-start"
                  onToggle={() =>
                    setNewLib({
                      ...newLib,
                      tmdb_store_in_media_dir: !newLib.tmdb_store_in_media_dir,
                    })
                  }
                />
                <AdminToggleButton
                  checked={newLib.tmdb_sync_on_new_media}
                  label="Auto TMDB sync when scan finds new media"
                  className="w-full justify-start"
                  onToggle={() =>
                    setNewLib({
                      ...newLib,
                      tmdb_sync_on_new_media: !newLib.tmdb_sync_on_new_media,
                    })
                  }
                />
                <AdminToggleButton
                  checked={newLib.tmdb_fetch_metadata}
                  label="Fetch metadata fields (title, overview, rating)"
                  className="w-full justify-start"
                  onToggle={() =>
                    setNewLib({ ...newLib, tmdb_fetch_metadata: !newLib.tmdb_fetch_metadata })
                  }
                />
                <AdminToggleButton
                  checked={newLib.tmdb_fetch_posters}
                  label="Fetch poster images"
                  className="w-full justify-start"
                  onToggle={() =>
                    setNewLib({ ...newLib, tmdb_fetch_posters: !newLib.tmdb_fetch_posters })
                  }
                />
                <AdminToggleButton
                  checked={newLib.tmdb_fetch_backdrops}
                  label="Fetch backdrop/banner images"
                  className="w-full justify-start"
                  onToggle={() =>
                    setNewLib({ ...newLib, tmdb_fetch_backdrops: !newLib.tmdb_fetch_backdrops })
                  }
                />
                <AdminToggleButton
                  checked={newLib.tmdb_fetch_reviews}
                  label="Fetch TMDB reviews"
                  className="w-full justify-start"
                  onToggle={() =>
                    setNewLib({ ...newLib, tmdb_fetch_reviews: !newLib.tmdb_fetch_reviews })
                  }
                />
                <label className="panel-soft flex items-center gap-2 px-3 py-2 text-sm md:col-span-2 xl:col-span-3">
                  <span className="w-40 shrink-0">Scheduled TMDB sync</span>
                  <select
                    value={newLib.tmdb_sync_schedule}
                    onChange={(e) =>
                      setNewLib({
                        ...newLib,
                        tmdb_sync_schedule: e.target.value as
                          | 'manual'
                          | 'hourly'
                          | 'daily'
                          | 'weekly'
                          | 'monthly',
                      })
                    }
                    className="select flex-1 px-3 py-2 text-sm"
                  >
                    <option value="manual">Manual only</option>
                    <option value="hourly">Every hour</option>
                    <option value="daily">Every 24 hours</option>
                    <option value="weekly">Every week</option>
                    <option value="monthly">Every month</option>
                  </select>
                </label>
              </div>
            </form>
          </section>

          <section className="space-y-4">
            <h2 className="text-xl font-semibold">Manage Libraries</h2>
            {libraries.length === 0 ? (
              <div className="panel-soft px-4 py-3">
                <p className="text-sm muted">No libraries configured.</p>
              </div>
            ) : (
              <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-3">
                {libraries.map((lib) => (
                  <div key={lib.id} data-admin-library-card-id={lib.id} className="tile space-y-3 p-4">
                    <form
                      onSubmit={(e) => {
                        e.preventDefault();
                        void saveLibrary(lib.id);
                      }}
                      className="space-y-2"
                    >
                      <input
                        aria-label={`Library name ${lib.name}`}
                        value={libraryEdits[lib.id]?.name ?? lib.name}
                        onChange={(e) => setLibraryEdit(lib.id, 'name', e.target.value)}
                        className="input w-full px-3 py-2 text-sm"
                      />
                      <input
                        aria-label={`Library path ${lib.name}`}
                        value={libraryEdits[lib.id]?.path ?? lib.paths[0]?.path ?? ''}
                        onChange={(e) => setLibraryEdit(lib.id, 'path', e.target.value)}
                        className="input w-full px-3 py-2 text-sm"
                      />
                      <div className="flex gap-2">
                        <button
                          type="button"
                          onClick={() => browseExistingLibraryPath(lib.id)}
                          disabled={
                            hostDirBrowserOpen &&
                            hostDirBrowserLoading &&
                            hostDirBrowserTargetLibraryId === lib.id
                          }
                          className="btn-secondary flex-1 px-3 py-1.5 text-sm disabled:opacity-50"
                        >
                          {hostDirBrowserOpen &&
                          hostDirBrowserLoading &&
                          hostDirBrowserTargetLibraryId === lib.id
                            ? 'Loading...'
                            : 'Browse Host'}
                        </button>
                        <button
                          type="button"
                          onClick={() => scanLibrary(lib.id)}
                          className="btn-secondary flex-1 px-3 py-1.5 text-sm"
                        >
                          Scan
                        </button>
                        <button
                          type="button"
                          onClick={() => syncLibraryTmdb(lib.id)}
                          className="btn-secondary flex-1 px-3 py-1.5 text-sm"
                        >
                          TMDB Sync
                        </button>
                        <button type="submit" className="btn-primary flex-1 px-3 py-1.5 text-sm">
                          Save
                        </button>
                      </div>
                      <div className="space-y-1">
                        <AdminToggleButton
                          checked={libraryEdits[lib.id]?.show_images ?? lib.settings.show_images}
                          label="Enable artwork thumbnails"
                          className="w-full justify-start"
                          onToggle={() =>
                            setLibraryEdit(
                              lib.id,
                              'show_images',
                              !(libraryEdits[lib.id]?.show_images ?? lib.settings.show_images),
                            )
                          }
                        />
                        <AdminToggleButton
                          checked={
                            libraryEdits[lib.id]?.prefer_local_artwork ??
                            lib.settings.prefer_local_artwork
                          }
                          label="Prefer local artwork files"
                          className="w-full justify-start"
                          onToggle={() =>
                            setLibraryEdit(
                              lib.id,
                              'prefer_local_artwork',
                              !(
                                libraryEdits[lib.id]?.prefer_local_artwork ??
                                lib.settings.prefer_local_artwork
                              ),
                            )
                          }
                        />
                        <AdminToggleButton
                          checked={
                            libraryEdits[lib.id]?.fetch_online_artwork ??
                            lib.settings.fetch_online_artwork
                          }
                          label="Fetch missing artwork online"
                          className="w-full justify-start"
                          onToggle={() =>
                            setLibraryEdit(
                              lib.id,
                              'fetch_online_artwork',
                              !(
                                libraryEdits[lib.id]?.fetch_online_artwork ??
                                lib.settings.fetch_online_artwork
                              ),
                            )
                          }
                        />
                        <AdminToggleButton
                          checked={
                            libraryEdits[lib.id]?.tmdb_store_in_media_dir ??
                            lib.settings.tmdb_store_in_media_dir
                          }
                          label="Store TMDB images in media folders"
                          className="w-full justify-start"
                          onToggle={() =>
                            setLibraryEdit(
                              lib.id,
                              'tmdb_store_in_media_dir',
                              !(
                                libraryEdits[lib.id]?.tmdb_store_in_media_dir ??
                                lib.settings.tmdb_store_in_media_dir
                              ),
                            )
                          }
                        />
                        <AdminToggleButton
                          checked={
                            libraryEdits[lib.id]?.tmdb_sync_on_new_media ??
                            lib.settings.tmdb_sync_on_new_media
                          }
                          label="Auto TMDB sync when scan finds new media"
                          className="w-full justify-start"
                          onToggle={() =>
                            setLibraryEdit(
                              lib.id,
                              'tmdb_sync_on_new_media',
                              !(
                                libraryEdits[lib.id]?.tmdb_sync_on_new_media ??
                                lib.settings.tmdb_sync_on_new_media
                              ),
                            )
                          }
                        />
                        <AdminToggleButton
                          checked={
                            libraryEdits[lib.id]?.tmdb_fetch_metadata ??
                            lib.settings.tmdb_fetch_metadata
                          }
                          label="Fetch metadata fields (title, overview, rating)"
                          className="w-full justify-start"
                          onToggle={() =>
                            setLibraryEdit(
                              lib.id,
                              'tmdb_fetch_metadata',
                              !(
                                libraryEdits[lib.id]?.tmdb_fetch_metadata ??
                                lib.settings.tmdb_fetch_metadata
                              ),
                            )
                          }
                        />
                        <AdminToggleButton
                          checked={
                            libraryEdits[lib.id]?.tmdb_fetch_posters ??
                            lib.settings.tmdb_fetch_posters
                          }
                          label="Fetch poster images"
                          className="w-full justify-start"
                          onToggle={() =>
                            setLibraryEdit(
                              lib.id,
                              'tmdb_fetch_posters',
                              !(
                                libraryEdits[lib.id]?.tmdb_fetch_posters ??
                                lib.settings.tmdb_fetch_posters
                              ),
                            )
                          }
                        />
                        <AdminToggleButton
                          checked={
                            libraryEdits[lib.id]?.tmdb_fetch_backdrops ??
                            lib.settings.tmdb_fetch_backdrops
                          }
                          label="Fetch backdrop/banner images"
                          className="w-full justify-start"
                          onToggle={() =>
                            setLibraryEdit(
                              lib.id,
                              'tmdb_fetch_backdrops',
                              !(
                                libraryEdits[lib.id]?.tmdb_fetch_backdrops ??
                                lib.settings.tmdb_fetch_backdrops
                              ),
                            )
                          }
                        />
                        <AdminToggleButton
                          checked={
                            libraryEdits[lib.id]?.tmdb_fetch_reviews ??
                            lib.settings.tmdb_fetch_reviews
                          }
                          label="Fetch TMDB reviews"
                          className="w-full justify-start"
                          onToggle={() =>
                            setLibraryEdit(
                              lib.id,
                              'tmdb_fetch_reviews',
                              !(
                                libraryEdits[lib.id]?.tmdb_fetch_reviews ??
                                lib.settings.tmdb_fetch_reviews
                              ),
                            )
                          }
                        />
                        <label className="panel-soft flex items-center gap-2 px-3 py-2 text-sm">
                          <span className="w-36 shrink-0">Scheduled TMDB sync</span>
                          <select
                            value={
                              libraryEdits[lib.id]?.tmdb_sync_schedule ??
                              lib.settings.tmdb_sync_schedule
                            }
                            onChange={(e) =>
                              setLibraryEdit(
                                lib.id,
                                'tmdb_sync_schedule',
                                e.target.value as
                                  | 'manual'
                                  | 'hourly'
                                  | 'daily'
                                  | 'weekly'
                                  | 'monthly',
                              )
                            }
                            className="select flex-1 px-3 py-2 text-sm"
                          >
                            <option value="manual">Manual only</option>
                            <option value="hourly">Every hour</option>
                            <option value="daily">Every 24 hours</option>
                            <option value="weekly">Every week</option>
                            <option value="monthly">Every month</option>
                          </select>
                        </label>
                        <p className="px-1 text-xs muted">
                          Last TMDB sync:{' '}
                          {lib.settings.tmdb_last_sync_ts
                            ? new Date(lib.settings.tmdb_last_sync_ts * 1000).toLocaleString()
                            : 'never'}
                        </p>
                      </div>
                    </form>
                    {lib.kind === 'music' && (
                      <form
                        onSubmit={(e) => {
                          e.preventDefault();
                          void importMusicToLibrary(lib.id);
                        }}
                        className="panel-soft space-y-2 rounded-lg border border-[var(--border)] px-3 py-3"
                      >
                        <p className="text-sm font-semibold">Import Song From YouTube</p>
                        <p className="text-xs muted">
                          Keep artist and title separate. Example: artist = Tory Lanez, title = The
                          Color Violet.
                        </p>
                        <input
                          value={getMusicImportState(lib.id).source}
                          onChange={(e) => setMusicImportField(lib.id, 'source', e.target.value)}
                          placeholder="YouTube URL or 11-character video ID"
                          className="input w-full px-3 py-2 text-sm"
                          required
                        />
                        <div className="grid grid-cols-1 gap-2 sm:grid-cols-3">
                          <input
                            value={getMusicImportState(lib.id).artist}
                            onChange={(e) => setMusicImportField(lib.id, 'artist', e.target.value)}
                            placeholder="Artist"
                            className="input w-full px-3 py-2 text-sm"
                            required
                          />
                          <input
                            value={getMusicImportState(lib.id).album}
                            onChange={(e) => setMusicImportField(lib.id, 'album', e.target.value)}
                            placeholder="Album (defaults to Singles)"
                            className="input w-full px-3 py-2 text-sm"
                          />
                          <input
                            value={getMusicImportState(lib.id).title}
                            onChange={(e) => setMusicImportField(lib.id, 'title', e.target.value)}
                            placeholder="Song title"
                            className="input w-full px-3 py-2 text-sm"
                            required
                          />
                        </div>
                        <div className="flex items-center justify-end">
                          <button
                            type="submit"
                            className="btn-primary px-3 py-1.5 text-sm disabled:opacity-60"
                            disabled={getMusicImportState(lib.id).importing}
                          >
                            {getMusicImportState(lib.id).importing
                              ? 'Importing...'
                              : 'Download + Add'}
                          </button>
                        </div>
                      </form>
                    )}
                    <div className="flex items-center justify-between gap-3 border-t border-[var(--border)] pt-3">
                      <p className="text-sm muted">
                        {lib.kind} · {lib.item_count} items
                      </p>
                      <button
                        onClick={() => requestDeleteLibrary(lib.id)}
                        className="btn-ghost px-3 py-1.5 text-sm text-[var(--danger)]"
                      >
                        Delete
                      </button>
                    </div>
                  </div>
                ))}
              </div>
            )}
          </section>
        </div>
      )}

      {activeTab === 'channels' && (
        <div className="space-y-8">
          <section className="panel space-y-4 p-6">
            <h2 className="text-xl font-semibold">Create Channel</h2>
            <form onSubmit={createChannel} className="space-y-4">
              <div className="grid grid-cols-1 gap-3 md:grid-cols-[1.4fr_1fr_auto]">
                <input
                  placeholder="Channel name"
                  value={newChannel.name}
                  onChange={(e) => setNewChannel({ ...newChannel, name: e.target.value })}
                  className="input px-3 py-2 text-sm"
                  required
                />
                <select
                  aria-label="Channel kind"
                  value={newChannel.kind}
                  onChange={(e) =>
                    setNewChannel({ ...newChannel, kind: e.target.value as 'text' | 'voice' })
                  }
                  className="select px-3 py-2 text-sm"
                >
                  <option value="text">Text</option>
                  <option value="voice">Voice</option>
                </select>
                <button type="submit" className="btn-primary px-4 py-2 text-sm">
                  Create
                </button>
              </div>
              <AdminToggleButton
                checked={newChannel.is_private}
                label="Private channel (admins only)"
                className="w-fit justify-start"
                onToggle={() => setNewChannel({ ...newChannel, is_private: !newChannel.is_private })}
              />
            </form>
          </section>

          <section className="space-y-4">
            <h2 className="text-xl font-semibold">Manage Channels</h2>
            {channels.length === 0 ? (
              <div className="panel-soft px-4 py-3">
                <p className="text-sm muted">No channels available.</p>
              </div>
            ) : (
              <div className="space-y-3">
                {channels.map((channel) => {
                  const edit = channelEdits[channel.id] || toChannelEditState(channel);
                  const creatorName = usersById.get(channel.created_by)?.username || channel.created_by;
                  return (
                    <div key={channel.id} data-admin-channel-card-id={channel.id} className="tile space-y-3 p-4">
                      <div className="flex flex-wrap items-start justify-between gap-2">
                        <div className="flex items-center gap-2">
                          <span className="chip">{channel.kind}</span>
                          <span className="chip">#{channel.position}</span>
                          <span className="chip">{edit.is_private ? 'Admins only' : 'All users'}</span>
                        </div>
                        <p className="text-xs muted">
                          Created by {creatorName} · {new Date(channel.created_ts * 1000).toLocaleString()}
                        </p>
                      </div>
                      <div className="grid grid-cols-1 gap-3 md:grid-cols-[1.4fr_auto_auto]">
                        <input
                          aria-label={`Channel name ${channel.name}`}
                          value={edit.name}
                          onChange={(e) => setChannelEdit(channel.id, 'name', e.target.value)}
                          className="input w-full px-3 py-2 text-sm"
                        />
                        <AdminToggleButton
                          checked={edit.is_private}
                          label="Private"
                          className="w-fit justify-start"
                          onToggle={() =>
                            setChannelEdit(channel.id, 'is_private', !edit.is_private)
                          }
                        />
                        <div className="flex gap-2">
                          <button
                            onClick={() => saveChannel(channel.id)}
                            className="btn-primary flex-1 px-3 py-1.5 text-sm"
                          >
                            Save
                          </button>
                          <button
                            onClick={() => requestDeleteChannel(channel.id)}
                            className="btn-ghost px-3 py-1.5 text-sm text-[var(--danger)]"
                          >
                            Delete
                          </button>
                        </div>
                      </div>
                    </div>
                  );
                })}
              </div>
            )}
          </section>
        </div>
      )}

      {activeTab === 'rooms' && (
        <div className="space-y-4">
          <h2 className="text-xl font-semibold">Manage Rooms</h2>
          {rooms.length === 0 ? (
            <div className="panel-soft rounded-xl px-4 py-3 text-sm muted">
              No rooms available.
            </div>
          ) : (
            <div className="space-y-3">
              {rooms.map((room) => {
                const edit = roomEdits[room.room_id] || toRoomEditState(room);
                return (
                  <div key={room.room_id} data-admin-room-card-id={room.room_id} className="tile space-y-3 p-4">
                    <div className="flex flex-wrap items-center justify-between gap-2">
                      <p className="font-semibold">{room.title}</p>
                      <div className="flex items-center gap-2">
                        <span className="chip">{room.room_mode}</span>
                        <span className="chip">{room.status}</span>
                        <span className="chip">{room.invite_only ? 'Private' : 'Public'}</span>
                      </div>
                    </div>
                    <p className="text-xs muted">
                      Host: {room.host_username} · Members: {room.member_count}
                      {room.password_required ? ' · Password protected' : ''}
                    </p>
                    <p className="text-xs muted">
                      Created {new Date(room.created_ts * 1000).toLocaleString()}
                    </p>
                    <div className="grid grid-cols-1 gap-2 md:grid-cols-[1fr_auto]">
                      <input
                        aria-label={`Room name ${room.room_id}`}
                        value={edit.room_name}
                        onChange={(e) => setRoomEdit(room.room_id, 'room_name', e.target.value)}
                        className="input w-full px-3 py-2 text-sm"
                        placeholder="Room name"
                      />
                      <div className="flex flex-wrap gap-2">
                        <button
                          onClick={() => saveRoomName(room.room_id)}
                          className="btn-primary px-3 py-1.5 text-sm"
                        >
                          Save Name
                        </button>
                        <button
                          onClick={() => requestEndRoom(room.room_id)}
                          disabled={room.status === 'ended'}
                          className="btn-secondary px-3 py-1.5 text-sm disabled:opacity-50"
                        >
                          End
                        </button>
                        <button
                          onClick={() => requestDeleteRoom(room.room_id)}
                          className="btn-ghost px-3 py-1.5 text-sm text-[var(--danger)]"
                        >
                          Delete
                        </button>
                      </div>
                    </div>
                    <p className="text-xs muted break-all">Room ID: {room.room_id}</p>
                  </div>
                );
              })}
            </div>
          )}
        </div>
      )}

      {activeTab === 'ai' && (
        <div className="space-y-6">
          <section className="panel space-y-4 p-6">
            <div className="flex flex-col gap-2 md:flex-row md:items-start md:justify-between">
              <div>
                <h2 className="text-xl font-semibold">AI Models</h2>
                <p className="text-sm muted">
                  Manage installed GGUF models and the server-side folder Rustyfin uses for AI
                  model storage.
                </p>
              </div>
              <span
                className={`chip ${
                  aiAdminState?.available
                    ? 'chip-accent'
                    : 'border-[var(--border)] text-[var(--text-muted)]'
                }`}
              >
                {aiAdminState?.available ? 'Inference available' : 'Inference unavailable'}
              </span>
            </div>

            {aiAdminLoading && !aiAdminState ? (
              <p className="text-sm muted">Loading AI configuration…</p>
            ) : (
              <>
                <div className="grid gap-4 xl:grid-cols-[1.2fr,1fr]">
                  <div className="panel-soft space-y-4 rounded-2xl border border-[var(--border)] p-4">
                    <div className="space-y-1">
                      <p className="text-sm font-semibold">Model Storage Folder</p>
                      <p className="text-xs muted">
                        Current source:{' '}
                        <span className="font-medium text-[var(--text-main)]">
                          {aiAdminState ? titleCase(aiAdminState.model_dir_source) : '—'}
                        </span>
                      </p>
                      {aiAdminState && !aiAdminState.model_storage_available && (
                        <p className="text-xs text-[var(--danger)]">
                          {aiAdminState.model_storage_error ||
                            'Rustyfin cannot read the active AI model folder right now.'}
                        </p>
                      )}
                    </div>

                    <div className="space-y-2">
                      <label className="text-xs font-semibold uppercase tracking-[0.12em] muted">
                        Model Directory
                      </label>
                      <div className="flex flex-col gap-2 sm:flex-row">
                        <input
                          value={aiModelDirInput}
                          onChange={(e) => setAiModelDirInput(e.target.value)}
                          placeholder={aiAdminState?.default_model_dir || '/var/lib/rustyfin/ai/models'}
                          className="input flex-1 px-3 py-2 text-sm font-mono"
                        />
                        <button
                          type="button"
                          onClick={browseAiModelDirectory}
                          className="btn-secondary px-4 py-2 text-sm"
                        >
                          Browse
                        </button>
                      </div>
                    </div>

                    <div className="flex flex-wrap gap-2">
                      <button
                        type="button"
                        onClick={() => {
                          void saveAiModelDirectory();
                        }}
                        disabled={savingAiModelDir}
                        className="btn-primary px-4 py-2 text-sm disabled:opacity-50"
                      >
                        Save folder
                      </button>
                      <button
                        type="button"
                        onClick={() => {
                          void resetAiModelDirectory();
                        }}
                        disabled={savingAiModelDir}
                        className="btn-ghost px-4 py-2 text-sm disabled:opacity-50"
                      >
                        Use default
                      </button>
                    </div>

                    <div className="text-xs muted space-y-1">
                      <p>
                        Active path:{' '}
                        <span className="font-mono text-[var(--text-main)]">
                          {aiAdminState?.model_dir || '—'}
                        </span>
                      </p>
                      <p>
                        Rustyfin AI default:{' '}
                        <span className="font-mono text-[var(--text-main)]">
                          {aiAdminState?.default_model_dir || '/var/lib/rustyfin/ai/models'}
                        </span>
                      </p>
                    </div>
                  </div>

                  <div className="panel-soft space-y-4 rounded-2xl border border-[var(--border)] p-4">
                    <div className="space-y-1">
                      <p className="text-sm font-semibold">Download Model From Link</p>
                      <p className="text-xs muted">
                        Paste a direct `.gguf` URL. Rustyfin will download it into the active model
                        folder.
                      </p>
                    </div>

                    <div className="space-y-2">
                      <input
                        value={aiModelPullUrl}
                        onChange={(e) => setAiModelPullUrl(e.target.value)}
                        placeholder="https://…/model.gguf"
                        className="input w-full px-3 py-2 text-sm"
                      />
                      <div className="flex flex-wrap gap-2">
                        {aiModelPullState?.active ? (
                          <button
                            type="button"
                            onClick={cancelAiModelPull}
                            className="btn-danger px-4 py-2 text-sm"
                          >
                            Stop download
                          </button>
                        ) : (
                          <button
                            type="button"
                            onClick={pullAiModel}
                            disabled={!aiModelPullUrl.trim()}
                            className="btn-primary px-4 py-2 text-sm disabled:opacity-50"
                          >
                            Download model
                          </button>
                        )}
                      </div>
                    </div>

                    {aiModelPullState && (
                      <div className="space-y-2">
                        <div className="flex items-center justify-between gap-3 text-xs">
                          <span className={aiModelPullState.error ? 'text-red-300' : 'muted'}>
                            {aiModelPullState.error || aiModelPullState.status}
                          </span>
                          <span className="font-medium text-[var(--text-main)]">
                            {aiModelPullState.active ? `${aiModelPullState.percent}%` : ''}
                            {aiModelPullState.done ? 'Done' : ''}
                          </span>
                        </div>
                        {(aiModelPullState.active || aiModelPullState.done) && (
                          <div className="rf-progress-track">
                            <div
                              className="rf-progress-fill transition-all duration-300"
                              style={{ width: `${aiModelPullState.percent}%` }}
                            />
                          </div>
                        )}
                      </div>
                    )}
                  </div>
                </div>

                <div className="grid gap-4 xl:grid-cols-2">
                  <div className="panel-soft space-y-4 rounded-2xl border border-[var(--border)] p-4">
                    <div className="flex items-start justify-between gap-3">
                      <div>
                        <p className="text-sm font-semibold">Remote Backend</p>
                        <p className="text-xs muted">
                          Optional OpenAI-compatible planner or overload-fallback provider.
                        </p>
                      </div>
                      <span
                        className={`chip ${
                          aiRemoteBackendInput.enabled && aiRemoteBackendInput.base_url.trim()
                            ? 'chip-accent'
                            : 'border-[var(--border)] text-[var(--text-muted)]'
                        }`}
                      >
                        {aiRemoteBackendInput.enabled && aiRemoteBackendInput.base_url.trim()
                          ? 'Configured'
                          : 'Disabled'}
                      </span>
                    </div>

                    <div className="grid gap-3 md:grid-cols-2">
                      <label className="space-y-1 text-xs md:col-span-2">
                        <span className="font-semibold uppercase tracking-[0.12em] muted">
                          Base URL
                        </span>
                        <input
                          value={aiRemoteBackendInput.base_url}
                          onChange={(event) =>
                            setAiRemoteBackendInput((current) => ({
                              ...current,
                              base_url: event.target.value,
                            }))
                          }
                          placeholder="https://api.example.com/v1"
                          className="input w-full px-3 py-2 text-sm"
                        />
                      </label>
                      <label className="space-y-1 text-xs">
                        <span className="font-semibold uppercase tracking-[0.12em] muted">
                          Model
                        </span>
                        <input
                          value={aiRemoteBackendInput.model}
                          onChange={(event) =>
                            setAiRemoteBackendInput((current) => ({
                              ...current,
                              model: event.target.value,
                            }))
                          }
                          placeholder="gpt-4o-mini"
                          className="input w-full px-3 py-2 text-sm"
                        />
                      </label>
                      <label className="space-y-1 text-xs">
                        <span className="font-semibold uppercase tracking-[0.12em] muted">
                          API Key Env
                        </span>
                        <input
                          value={aiRemoteBackendInput.api_key_env}
                          onChange={(event) =>
                            setAiRemoteBackendInput((current) => ({
                              ...current,
                              api_key_env: event.target.value,
                            }))
                          }
                          placeholder="RUSTFIN_REMOTE_AI_KEY"
                          className="input w-full px-3 py-2 text-sm font-mono"
                        />
                      </label>
                      <label className="space-y-1 text-xs">
                        <span className="font-semibold uppercase tracking-[0.12em] muted">
                          Timeout Secs
                        </span>
                        <input
                          type="number"
                          min={1}
                          step={1}
                          value={aiRemoteBackendInput.timeout_secs}
                          onChange={(event) =>
                            setAiRemoteBackendInput((current) => ({
                              ...current,
                              timeout_secs: Number(event.target.value) || 120,
                            }))
                          }
                          className="input w-full px-3 py-2 text-sm"
                        />
                      </label>
                      <label className="space-y-1 text-xs">
                        <span className="font-semibold uppercase tracking-[0.12em] muted">
                          Max Parallel
                        </span>
                        <input
                          type="number"
                          min={1}
                          step={1}
                          value={aiRemoteBackendInput.max_parallel_requests}
                          onChange={(event) =>
                            setAiRemoteBackendInput((current) => ({
                              ...current,
                              max_parallel_requests: Number(event.target.value) || 1,
                            }))
                          }
                          className="input w-full px-3 py-2 text-sm"
                        />
                      </label>
                      <label className="space-y-1 text-xs md:col-span-2">
                        <span className="font-semibold uppercase tracking-[0.12em] muted">
                          Route Roles
                        </span>
                        <input
                          value={aiRemoteBackendInput.route_roles.join(', ')}
                          onChange={(event) =>
                            setAiRemoteBackendInput((current) => ({
                              ...current,
                              route_roles: event.target.value
                                .split(',')
                                .map((role) => role.trim().toLowerCase())
                                .filter(Boolean),
                            }))
                          }
                          placeholder="planner, all"
                          className="input w-full px-3 py-2 text-sm font-mono"
                        />
                      </label>
                    </div>

                    <div className="flex flex-wrap gap-2 text-sm">
                      <AdminToggleButton
                        checked={aiRemoteBackendInput.enabled}
                        label="Enabled"
                        onToggle={() =>
                          setAiRemoteBackendInput((current) => ({
                            ...current,
                            enabled: !current.enabled,
                          }))
                        }
                      />
                      <AdminToggleButton
                        checked={aiRemoteBackendInput.supports_prompt_cache}
                        label="Prompt cache"
                        onToggle={() =>
                          setAiRemoteBackendInput((current) => ({
                            ...current,
                            supports_prompt_cache: !current.supports_prompt_cache,
                          }))
                        }
                      />
                      <AdminToggleButton
                        checked={aiRemoteBackendInput.supports_structured_output}
                        label="Structured output"
                        onToggle={() =>
                          setAiRemoteBackendInput((current) => ({
                            ...current,
                            supports_structured_output: !current.supports_structured_output,
                          }))
                        }
                      />
                      <AdminToggleButton
                        checked={aiRemoteBackendInput.overload_fallback}
                        label="Overload fallback"
                        onToggle={() =>
                          setAiRemoteBackendInput((current) => ({
                            ...current,
                            overload_fallback: !current.overload_fallback,
                          }))
                        }
                      />
                    </div>

                    <div className="flex flex-wrap gap-2">
                      <button
                        type="button"
                        onClick={() => {
                          void saveAiRemoteBackend();
                        }}
                        disabled={savingAiRemoteBackend}
                        className="btn-primary px-4 py-2 text-sm disabled:opacity-50"
                      >
                        {savingAiRemoteBackend ? 'Saving…' : 'Save backend'}
                      </button>
                    </div>

                    {aiAdminState?.remote_backend ? (
                      <div className="rounded-xl border border-[var(--border)] bg-[var(--panel)]/65 px-4 py-3 text-xs muted">
                        <p className="font-semibold text-[var(--text-main)]">Persisted backend</p>
                        <p className="mt-1">
                          {aiAdminState.remote_backend.enabled ? 'Enabled' : 'Disabled'} ·{' '}
                          {aiAdminState.remote_backend.base_url || 'No URL'} ·{' '}
                          {aiAdminState.remote_backend.model || 'No model'}
                        </p>
                        <p className="mt-1">
                          Roles: {aiAdminState.remote_backend.route_roles.join(', ') || 'none'}
                        </p>
                      </div>
                    ) : (
                      <p className="text-xs muted">No remote backend is currently stored.</p>
                    )}
                  </div>

                  <div className="panel-soft space-y-4 rounded-2xl border border-[var(--border)] p-4">
                    <div className="flex items-start justify-between gap-3">
                      <div>
                        <p className="text-sm font-semibold">Benchmark & Profiles</p>
                        <p className="text-xs muted">
                          Run a host-specific benchmark sweep and inspect the stored recommendations.
                        </p>
                      </div>
                      <span className="chip chip-accent">
                        {aiAdminState?.scheduler?.overload_state
                          ? titleCase(aiAdminState.scheduler.overload_state)
                          : 'Idle'}
                      </span>
                    </div>

                    <div className="grid gap-3 md:grid-cols-2">
                      <label className="space-y-1 text-xs">
                        <span className="font-semibold uppercase tracking-[0.12em] muted">
                          Model
                        </span>
                        <select
                          value={aiBenchmarkModelName}
                          onChange={(event) => setAiBenchmarkModelName(event.target.value)}
                          className="input w-full px-3 py-2 text-sm"
                        >
                          <option value="">Choose a model</option>
                          {(aiAdminState?.models ?? []).map((model) => (
                            <option key={model.name} value={model.name}>
                              {model.name}
                            </option>
                          ))}
                        </select>
                      </label>
                      <label className="space-y-1 text-xs">
                        <span className="font-semibold uppercase tracking-[0.12em] muted">
                          Benchmark Label
                        </span>
                        <input
                          value={aiBenchmarkLabel}
                          onChange={(event) => setAiBenchmarkLabel(event.target.value)}
                          placeholder="admin-benchmark"
                          className="input w-full px-3 py-2 text-sm font-mono"
                        />
                      </label>
                    </div>

                    <div className="flex flex-wrap gap-2">
                      <button
                        type="button"
                        onClick={() => {
                          void runAiBenchmark();
                        }}
                        disabled={runningAiBenchmark || !aiBenchmarkModelName.trim()}
                        className="btn-primary px-4 py-2 text-sm disabled:opacity-50"
                      >
                        {runningAiBenchmark ? 'Benchmarking…' : 'Run benchmark'}
                      </button>
                    </div>

                    <div className="grid gap-4 md:grid-cols-2">
                      <div className="space-y-2 rounded-xl border border-[var(--border)] bg-[var(--panel)]/65 px-4 py-3">
                        <div className="flex items-center justify-between gap-3">
                          <p className="text-xs font-semibold uppercase tracking-[0.12em] muted">
                            Scheduler Snapshot
                          </p>
                          <span className="text-[11px] muted">
                            {aiAdminState?.scheduler
                              ? `${aiAdminState.scheduler.active_turns}/${aiAdminState.scheduler.max_concurrent_turns}`
                              : '—'}
                          </span>
                        </div>
                        {aiAdminState?.scheduler ? (
                          <div className="space-y-1 text-xs muted">
                            <p>Queue limit {aiAdminState.scheduler.queue_limit}</p>
                            <p>Queued turns {aiAdminState.scheduler.queued_turns}</p>
                            <p>
                              Warm pool {formatBytes(aiAdminState.scheduler.warm_pool_bytes)} /{' '}
                              {formatBytes(aiAdminState.scheduler.warm_pool_budget_bytes)}
                            </p>
                            <p>Rejected {aiAdminState.scheduler.rejected_turns_total}</p>
                            <p>Degraded {aiAdminState.scheduler.degraded_turns_total}</p>
                            <p>
                              Hot models:{' '}
                              {aiAdminState.scheduler.warm_models.length > 0
                                ? aiAdminState.scheduler.warm_models
                                    .slice(0, 3)
                                    .map((model) => model.model_name)
                                    .join(', ')
                                : 'none'}
                            </p>
                          </div>
                        ) : (
                          <p className="text-xs muted">No scheduler snapshot available yet.</p>
                        )}
                      </div>

                      <div className="space-y-2 rounded-xl border border-[var(--border)] bg-[var(--panel)]/65 px-4 py-3">
                        <div className="flex items-center justify-between gap-3">
                          <p className="text-xs font-semibold uppercase tracking-[0.12em] muted">
                            Recommendation
                          </p>
                          <span className="text-[11px] muted">
                            {aiAdminState?.model_profiles?.[0]?.benchmark_count ?? 0} runs
                          </span>
                        </div>
                        {aiAdminState?.model_profiles?.[0] ? (
                          <div className="space-y-1 text-xs muted">
                            <p className="font-medium text-[var(--text-main)]">
                              {aiAdminState.model_profiles[0].model_name}
                            </p>
                            <p>
                              {aiAdminState.model_profiles[0].recommended_n_threads} threads ·{' '}
                              {aiAdminState.model_profiles[0].recommended_n_gpu_layers} GPU layers ·{' '}
                              {aiAdminState.model_profiles[0].recommended_split_mode}
                            </p>
                            <p>
                              Caps: planner {aiAdminState.model_profiles[0].planner_max_output} ·
                              summary {aiAdminState.model_profiles[0].summary_max_output} · completion{' '}
                              {aiAdminState.model_profiles[0].preferred_completion_tokens}
                            </p>
                            <p>
                              Warmup {titleCase(aiAdminState.model_profiles[0].warmup_cost_class)} · last benchmark{' '}
                              {aiAdminState.model_profiles[0].last_benchmark_label}
                            </p>
                            <p>
                              Throughput {aiAdminState.model_profiles[0].last_tokens_per_second.toFixed(1)} t/s ·
                              estimated {formatBytes(aiAdminState.model_profiles[0].estimated_model_bytes)}
                            </p>
                          </div>
                        ) : (
                          <p className="text-xs muted">No profile recommendation has been stored yet.</p>
                        )}
                      </div>
                    </div>

                    <div className="space-y-2 rounded-xl border border-[var(--border)] bg-[var(--panel)]/65 px-4 py-3">
                      <div className="flex items-center justify-between gap-3">
                        <p className="text-xs font-semibold uppercase tracking-[0.12em] muted">
                          Active Role Routing
                        </p>
                        <span className="text-[11px] muted">
                          {aiAdminState?.role_routing?.length ?? 0} roles
                        </span>
                      </div>
                      {aiAdminState?.role_routing && aiAdminState.role_routing.length > 0 ? (
                        <div className="space-y-2">
                          {aiAdminState.role_routing.map((route) => (
                            <div
                              key={`${route.role}-${route.model_name}-${route.backend_id}`}
                              className="rounded-lg border border-[var(--border)]/70 px-3 py-2 text-xs muted"
                            >
                              <div className="flex flex-wrap items-center gap-2">
                                <span className="chip border-[var(--border)] text-[var(--text-muted)]">
                                  {route.role}
                                </span>
                                <span className="font-medium text-[var(--text-main)]">
                                  {route.model_name}
                                </span>
                                <span>{route.backend_kind}</span>
                                <span>{route.selection_source.replaceAll('_', ' ')}</span>
                                <span>{aiRecommendationStatusLabel(route.recommendation_status)}</span>
                              </div>
                              {route.recommendation_note ? (
                                <p className="mt-1 text-[11px] muted">{route.recommendation_note}</p>
                              ) : null}
                            </div>
                          ))}
                        </div>
                      ) : (
                        <p className="text-xs muted">
                          No routed role selections have been recorded yet for this runtime.
                        </p>
                      )}
                    </div>

                    {aiAdminState?.model_benchmarks && aiAdminState.model_benchmarks.length > 0 ? (
                      <div className="space-y-2">
                        <p className="text-xs font-semibold uppercase tracking-[0.12em] muted">
                          Recent Benchmarks
                        </p>
                        <div className="space-y-2">
                          {aiAdminState.model_benchmarks.slice(0, 4).map((benchmark) => (
                            <div
                              key={benchmark.id}
                              className="rounded-xl border border-[var(--border)] bg-[var(--panel)]/65 px-4 py-3 text-xs muted"
                            >
                              <div className="flex flex-wrap items-center justify-between gap-2">
                                <p className="font-medium text-[var(--text-main)]">
                                  {benchmark.model_name} · {benchmark.benchmark_label}
                                </p>
                                <span>{formatTs(benchmark.updated_ts)}</span>
                              </div>
                              <p className="mt-1">
                                {benchmark.n_threads} threads · {benchmark.n_gpu_layers} GPU layers ·{' '}
                                {benchmark.split_mode}
                              </p>
                              <p className="mt-1">
                                Load {benchmark.load_duration_ms}ms · prefill {benchmark.prefill_duration_ms}ms ·
                                decode {benchmark.decode_duration_ms}ms · {benchmark.tokens_per_second.toFixed(1)} t/s
                              </p>
                              {benchmark.failure_message ? (
                                <p className="mt-1 text-[var(--danger)]">{benchmark.failure_message}</p>
                              ) : null}
                            </div>
                          ))}
                        </div>
                      </div>
                    ) : (
                      <p className="text-xs muted">No benchmark runs have been recorded yet.</p>
                    )}
                  </div>
                </div>

                <div className="panel-soft rounded-2xl border border-[var(--border)] p-4">
                  <div className="mb-3 flex items-center justify-between gap-3">
                    <div>
                      <p className="text-sm font-semibold">Installed Models</p>
                      <p className="text-xs muted">
                        {aiAdminState?.models.length ?? 0} model
                        {(aiAdminState?.models.length ?? 0) === 1 ? '' : 's'} detected in the
                        active AI folder.
                      </p>
                    </div>
                    <button
                      type="button"
                      onClick={() => {
                        void loadAiAdmin();
                      }}
                      className="btn-ghost px-3 py-1.5 text-sm"
                    >
                      Refresh
                    </button>
                  </div>

                  {!aiAdminState || aiAdminState.models.length === 0 ? (
                    <p className="text-sm muted">No `.gguf` models found in the active folder.</p>
                  ) : (
                    <div className="space-y-2">
                      {aiAdminState.models.map((model) => (
                        <div
                          key={model.name}
                          className="flex flex-col gap-3 rounded-xl border border-[var(--border)] bg-[var(--panel)]/65 px-4 py-3 sm:flex-row sm:items-center sm:justify-between"
                        >
                          <div className="min-w-0">
                            <p className="font-medium">{model.name}</p>
                            <p className="truncate text-xs muted" title={model.file}>
                              {model.file}
                            </p>
                            <p className="text-xs muted">{(model.size_gb || 0).toFixed(2)} GB</p>
                          </div>
                          <button
                            type="button"
                            onClick={() => {
                              void removeAiModel(model.name);
                            }}
                            disabled={aiDeletingModel === model.name}
                            className="btn-danger px-3 py-2 text-sm disabled:opacity-50"
                          >
                            {aiDeletingModel === model.name ? 'Removing…' : 'Delete'}
                          </button>
                        </div>
                      ))}
                    </div>
                  )}
                </div>

                <div className="panel-soft rounded-2xl border border-[var(--border)] p-4">
                  <div className="mb-3 flex items-center justify-between gap-3">
                    <div>
                      <p className="text-sm font-semibold">Assistant Audit Trail</p>
                      <p className="text-xs muted">
                        Recent persisted assistant requests, planned tools, execution summaries, and failure states.
                      </p>
                      {aiAdminState && (
                        <p className="mt-1 text-xs muted">
                          Retention: {aiAdminState.audit_retention_days} day
                          {aiAdminState.audit_retention_days === 1 ? '' : 's'}.
                          Automatic prune cadence: every{' '}
                          {formatUptime(aiAdminState.audit_prune_interval_seconds)}.
                        </p>
                      )}
                    </div>
                    <button
                      type="button"
                      onClick={() => {
                        void loadAiAdmin();
                      }}
                      className="btn-ghost px-3 py-1.5 text-sm"
                    >
                      Refresh
                    </button>
                  </div>

                  {aiAuditError ? (
                    <div className="notice-error rounded-xl px-4 py-3 text-sm">{aiAuditError}</div>
                  ) : aiAdminLoading && aiAuditEvents.length === 0 ? (
                    <p className="text-sm muted">Loading assistant audit…</p>
                  ) : aiAuditEvents.length === 0 ? (
                    <p className="text-sm muted">No persisted assistant requests yet.</p>
                  ) : (
                    <div className="space-y-3">
                      {aiAuditEvents.map((event) => (
                        <div
                          key={event.id}
                          className="rounded-xl border border-[var(--border)] bg-[var(--panel)]/65 px-4 py-3"
                        >
                          <div className="flex flex-col gap-2 sm:flex-row sm:items-start sm:justify-between">
                            <div className="min-w-0 space-y-1">
                              <div className="flex flex-wrap items-center gap-2">
                                <p className="font-medium">
                                  {event.username}
                                  <span className="ml-2 text-xs muted">{event.user_role}</span>
                                </p>
                                <span className="chip chip-accent">{titleCase(event.response_kind)}</span>
                              </div>
                              <p className="text-xs muted">
                                {formatTs(event.created_ts)} · model {event.model_name} · {event.history_len} prior
                                {event.history_len === 1 ? ' message' : ' messages'}
                              </p>
                              <p className="text-sm text-[var(--text-main)]">{event.message_preview}</p>
                            </div>
                            <p className="text-[11px] muted sm:text-right">trace {event.trace_id.slice(0, 8)}</p>
                          </div>

                          <div className="mt-3 grid gap-3 lg:grid-cols-[1fr,1.2fr]">
                            <div className="space-y-2">
                              <p className="text-[11px] font-semibold uppercase tracking-[0.12em] muted">
                                Planned Tools
                              </p>
                              {event.planned_tools.length === 0 ? (
                                <p className="text-xs muted">No grounded tools planned.</p>
                              ) : (
                                <div className="flex flex-wrap gap-2">
                                  {event.planned_tools.map((tool) => (
                                    <span key={tool} className="chip border-[var(--border)] text-[var(--text-muted)]">
                                      {tool}
                                    </span>
                                  ))}
                                </div>
                              )}

                              <p className="text-[11px] font-semibold uppercase tracking-[0.12em] muted">
                                Grounding Sources
                              </p>
                              {event.grounding_sources.length === 0 ? (
                                <p className="text-xs muted">No grounded sources recorded.</p>
                              ) : (
                                <div className="space-y-2">
                                  {event.grounding_sources.map((source) => (
                                    <div key={`${event.id}-${source.tool}-${source.label}`} className="text-xs muted">
                                      <span className="font-medium text-[var(--text-main)]">{source.label}</span>
                                      {' · '}
                                      {source.tool}
                                      {' · '}
                                      {source.status}
                                    </div>
                                  ))}
                                </div>
                              )}

                              <p className="text-[11px] font-semibold uppercase tracking-[0.12em] muted">
                                Grounding Chunks
                              </p>
                              {event.grounding_chunks.length === 0 ? (
                                <p className="text-xs muted">No compact grounding chunks recorded.</p>
                              ) : (
                                <div className="space-y-2">
                                  {event.grounding_chunks.map((chunk) => (
                                    <div
                                      key={chunk.id}
                                      className="rounded-lg border border-[var(--border)]/70 px-3 py-2 text-xs muted"
                                    >
                                      <div className="flex flex-wrap items-center gap-2">
                                        <span className="chip border-[var(--border)] text-[var(--text-muted)]">
                                          {chunk.source_kind}
                                        </span>
                                        <p className="font-medium text-[var(--text-main)]">{chunk.title}</p>
                                        <span className="text-[11px] muted">{chunk.id}</span>
                                      </div>
                                      <p className="mt-1">{chunk.excerpt}</p>
                                      {chunk.citation ? (
                                        <p className="mt-1 text-[11px] muted">
                                          {groundingCitationSummary(chunk.citation)}
                                        </p>
                                      ) : null}
                                    </div>
                                  ))}
                                </div>
                              )}
                            </div>

                            <div className="space-y-2">
                              <p className="text-[11px] font-semibold uppercase tracking-[0.12em] muted">
                                Planner
                              </p>
                              {event.planner && Object.keys(event.planner).length > 0 ? (
                                <div className="rounded-lg border border-[var(--border)]/70 px-3 py-2 text-xs muted">
                                  <div className="flex flex-wrap gap-2">
                                    {event.planner.planner_mode ? (
                                      <span className="chip border-[var(--border)] text-[var(--text-muted)]">
                                        {event.planner.planner_mode}
                                      </span>
                                    ) : null}
                                    {event.planner.fallback_reason ? (
                                      <span className="chip border-[var(--border)] text-[var(--text-muted)]">
                                        fallback {event.planner.fallback_reason}
                                      </span>
                                    ) : null}
                                    {event.planner.execution?.repair_attempts ? (
                                      <span className="chip border-[var(--border)] text-[var(--text-muted)]">
                                        repairs {event.planner.execution.repair_attempts}
                                      </span>
                                    ) : null}
                                  </div>
                                  {event.planner.raw_response_hash ? (
                                    <p className="mt-2 text-[11px] muted">
                                      raw {event.planner.raw_response_hash.slice(0, 12)}
                                    </p>
                                  ) : null}
                                  {event.planner.validation_errors?.length ? (
                                    <p className="mt-2 text-[11px] muted">
                                      {event.planner.validation_errors.join(' · ')}
                                    </p>
                                  ) : null}
                                  {event.planner.execution_trace ? (
                                    <div className="mt-2 space-y-1 text-[11px] muted">
                                      <p>
                                        stop {titleCase((event.planner.execution_trace.stop_reason ?? 'unknown').replaceAll('_', ' '))} ·{' '}
                                        {event.planner.execution_trace.attempts?.length ?? 0} attempts ·{' '}
                                        {event.planner.execution_trace.tool_step_count ?? 0} steps
                                      </p>
                                      <p>
                                        path {titleCase((event.planner.execution_trace.final_answer_path ?? 'none').replaceAll('_', ' '))} ·{' '}
                                        {event.planner.execution_trace.alternate_tool_count ?? 0} alternates ·{' '}
                                        {event.planner.execution_trace.recovery_step_count ?? 0} recovery
                                      </p>
                                      {event.planner.execution_trace.outcome_counts &&
                                      Object.keys(event.planner.execution_trace.outcome_counts).length > 0 ? (
                                        <p>
                                          outcomes{' '}
                                          {Object.entries(event.planner.execution_trace.outcome_counts)
                                            .map(([kind, count]) => `${kind}:${count}`)
                                            .join(' · ')}
                                        </p>
                                      ) : null}
                                    </div>
                                  ) : null}
                                </div>
                              ) : (
                                <p className="text-xs muted">No planner diagnostics recorded.</p>
                              )}

                              <p className="text-[11px] font-semibold uppercase tracking-[0.12em] muted">
                                Model Routing
                              </p>
                              {event.model_routing.length > 0 ? (
                                <div className="space-y-2">
                                  {event.model_routing.map((route) => (
                                    <div
                                      key={`${event.id}-${route.role}-${route.model_name}`}
                                      className="rounded-lg border border-[var(--border)]/70 px-3 py-2 text-xs muted"
                                    >
                                      <div className="flex flex-wrap items-center gap-2">
                                        <span className="chip border-[var(--border)] text-[var(--text-muted)]">
                                          {route.role}
                                        </span>
                                        <span className="font-medium text-[var(--text-main)]">
                                          {route.model_name}
                                        </span>
                                        <span>{route.backend_kind}</span>
                                        <span>{route.selection_source.replaceAll('_', ' ')}</span>
                                        <span>{aiRecommendationStatusLabel(route.recommendation_status)}</span>
                                      </div>
                                      {route.recommendation_note ? (
                                        <p className="mt-1 text-[11px] muted">{route.recommendation_note}</p>
                                      ) : null}
                                    </div>
                                  ))}
                                </div>
                              ) : (
                                <p className="text-xs muted">No role-routing diagnostics recorded.</p>
                              )}

                              <p className="text-[11px] font-semibold uppercase tracking-[0.12em] muted">
                                Executed Tools
                              </p>
                              {event.executed_tools.length === 0 ? (
                                <p className="text-xs muted">No tool execution recorded.</p>
                              ) : (
                                <div className="space-y-2">
                                  {event.executed_tools.map((tool) => (
                                    <div
                                      key={`${event.id}-${tool.tool}-${tool.input_summary}`}
                                      className="rounded-lg border border-[var(--border)]/70 px-3 py-2"
                                    >
                                      <div className="flex flex-wrap items-center justify-between gap-2">
                                        <p className="text-sm font-medium">{tool.tool}</p>
                                        <span className="text-[11px] muted">
                                          {tool.status}
                                          {tool.result_count != null ? ` · ${tool.result_count}` : ''}
                                        </span>
                                      </div>
                                      <p className="text-xs muted">{tool.label}</p>
                                      <p className="mt-1 truncate text-[11px] muted" title={tool.input_summary}>
                                        {tool.input_summary}
                                      </p>
                                    </div>
                                  ))}
                                </div>
                              )}
                            </div>
                          </div>

                          {event.error_message && (
                            <div className="mt-3 rounded-lg border border-red-400/25 bg-red-500/10 px-3 py-2 text-xs text-red-200">
                              {event.error_message}
                            </div>
                          )}
                        </div>
                      ))}
                    </div>
                  )}
                </div>
              </>
            )}
          </section>
        </div>
      )}

      {activeTab === 'server_logs' && (
        <div className="space-y-8">
          <section className="panel grid gap-6 p-6 xl:grid-cols-[22rem_minmax(0,1fr)]">
            <div className="space-y-4">
              <div>
                <h2 className="text-xl font-semibold">Minecraft Servers</h2>
                <p className="text-sm muted">
                  Detailed runtime diagnostics, lifecycle events, and journald output for managed Minecraft instances.
                </p>
              </div>

              <div className="space-y-2">
                {minecraftServers.length === 0 ? (
                  <div className="panel-soft rounded-xl px-4 py-3 text-sm muted">
                    No Minecraft servers exist yet.
                  </div>
                ) : (
                  minecraftServers.map((server) => {
                    const selected = server.id === selectedMinecraftServerId;
                    return (
                      <button
                        key={server.id}
                        type="button"
                        onClick={() => setSelectedMinecraftServerId(server.id)}
                        className={`w-full rounded-2xl p-[1px] text-left transition ${
                          selected
                            ? 'bg-[linear-gradient(90deg,var(--orange),var(--danger),var(--purple))]'
                            : 'border border-[var(--border)] bg-[var(--panel)]/55 hover:bg-[var(--surface)]/70'
                        }`}
                      >
                        <div
                          className={`rounded-[calc(1rem-1px)] px-4 py-3 ${
                            selected ? 'bg-[var(--surface)]/95' : ''
                          }`}
                        >
                          <div className="font-medium text-white">{server.display_name}</div>
                          <div className="mt-1 text-xs muted">
                            {server.server_distribution} {server.minecraft_version}
                            {' · '}
                            {server.world_name}
                            {' · '}
                            Port {server.listen_port}
                          </div>
                        </div>
                      </button>
                    );
                  })
                )}
              </div>
            </div>

            <div className="space-y-6">
              {selectedMinecraftServer ? (
                <>
                  <section className="panel-soft space-y-4 rounded-2xl p-5">
                    <div>
                      <h3 className="text-lg font-semibold text-white">{selectedMinecraftServer.display_name}</h3>
                      <p className="text-sm muted">
                        Host-facing diagnostics and runtime metadata for this server instance.
                      </p>
                    </div>

                    <div className="grid gap-3 text-sm sm:grid-cols-2 xl:grid-cols-3">
                      <div>
                        <div className="muted">Owner</div>
                        <div>{selectedMinecraftServer.owner_display_name}</div>
                      </div>
                      <div>
                        <div className="muted">Runtime</div>
                        <div>{titleCase(selectedMinecraftServer.runtime_mode)}</div>
                      </div>
                      <div>
                        <div className="muted">Health</div>
                        <div>{titleCase(selectedMinecraftServer.health_state)}</div>
                      </div>
                      <div>
                        <div className="muted">Version</div>
                        <div>{selectedMinecraftServer.server_distribution} {selectedMinecraftServer.minecraft_version}</div>
                      </div>
                      <div>
                        <div className="muted">World</div>
                        <div>{selectedMinecraftServer.world_name}</div>
                      </div>
                      <div>
                        <div className="muted">Gamemode</div>
                        <div>{titleCase(selectedMinecraftServer.gamemode)}</div>
                      </div>
                      <div>
                        <div className="muted">Difficulty</div>
                        <div>{titleCase(selectedMinecraftServer.difficulty)}</div>
                      </div>
                      <div>
                        <div className="muted">Port</div>
                        <div>{selectedMinecraftServer.listen_host}:{selectedMinecraftServer.listen_port}</div>
                      </div>
                      <div>
                        <div className="muted">Memory</div>
                        <div>{selectedMinecraftServer.min_memory_mb}-{selectedMinecraftServer.max_memory_mb} MB</div>
                      </div>
                      <div>
                        <div className="muted">Systemd unit</div>
                        <div className="break-all">{selectedMinecraftServer.systemd_unit_name}</div>
                      </div>
                      <div>
                        <div className="muted">Last started</div>
                        <div>{formatTs(selectedMinecraftServer.last_started_ts)}</div>
                      </div>
                      <div>
                        <div className="muted">Last stopped</div>
                        <div>{formatTs(selectedMinecraftServer.last_stopped_ts)}</div>
                      </div>
                      <div>
                        <div className="muted">Last ready</div>
                        <div>{formatTs(selectedMinecraftServer.last_ready_ts)}</div>
                      </div>
                      <div>
                        <div className="muted">Updated</div>
                        <div>{formatTs(selectedMinecraftServer.updated_ts)}</div>
                      </div>
                      <div>
                        <div className="muted">Advertised address</div>
                        <div>
                          {selectedMinecraftServer.advertised_host
                            ? `${selectedMinecraftServer.advertised_host}:${selectedMinecraftServer.advertised_port ?? selectedMinecraftServer.listen_port}`
                            : 'Not set'}
                        </div>
                      </div>
                      <div>
                        <div className="muted">Exit code</div>
                        <div>{selectedMinecraftServer.last_exit_code ?? 'Unknown'}</div>
                      </div>
                      <div className="sm:col-span-2 xl:col-span-3">
                        <div className="muted">Work directory</div>
                        <div className="break-all">{selectedMinecraftServer.server_work_dir}</div>
                      </div>
                      <div className="sm:col-span-2 xl:col-span-3">
                        <div className="muted">Instance root</div>
                        <div className="break-all">{selectedMinecraftServer.instance_root}</div>
                      </div>
                      <div className="sm:col-span-2 xl:col-span-3">
                        <div className="muted">Java path</div>
                        <div className="break-all">{selectedMinecraftServer.java_path}</div>
                      </div>
                      <div className="sm:col-span-2 xl:col-span-3">
                        <div className="muted">Last runtime error</div>
                        <div>{selectedMinecraftServer.last_error_summary || 'None'}</div>
                      </div>
                    </div>
                  </section>

                  <section className="grid gap-4 xl:grid-cols-2">
                    <div className="panel-soft flex min-h-0 flex-col gap-3 rounded-2xl p-5">
                      <div className="flex items-center justify-between gap-3">
                        <h3 className="text-sm font-semibold uppercase tracking-[0.18em] text-white/80">
                          Recent Events
                        </h3>
                        {minecraftServerEventsLoading ? (
                          <span className="chip text-[11px]">Refreshing</span>
                        ) : null}
                      </div>
                      {selectedMinecraftServerEvents.length === 0 ? (
                        <div className="panel rounded-xl px-4 py-3 text-sm muted">
                          No lifecycle events recorded yet.
                        </div>
                      ) : (
                        <div className="max-h-[26rem] space-y-3 overflow-y-auto pr-1">
                          {selectedMinecraftServerEvents.map((event) => (
                            <div
                              key={event.id}
                              className="rounded-xl border border-[var(--border)] bg-[var(--panel)]/55 px-4 py-3 text-sm"
                            >
                              <div className="flex items-start justify-between gap-3">
                                <div className="font-medium text-white">{event.message}</div>
                                <span className="chip text-[11px]">{titleCase(event.level)}</span>
                              </div>
                              <div className="mt-2 text-xs muted">
                                {titleCase(event.event_kind)} · {formatTs(event.created_ts)}
                              </div>
                            </div>
                          ))}
                        </div>
                      )}
                    </div>

                    <div className="panel-soft flex min-h-0 flex-col gap-3 rounded-2xl p-5">
                      <div className="flex items-center justify-between gap-3">
                        <h3 className="text-sm font-semibold uppercase tracking-[0.18em] text-white/80">
                          Journald Logs
                        </h3>
                        {minecraftServerLogsLoading ? (
                          <span className="chip text-[11px]">Refreshing</span>
                        ) : null}
                      </div>
                      {selectedMinecraftServerLogs.length === 0 ? (
                        <div className="panel rounded-xl px-4 py-3 text-sm muted">
                          No host logs have been returned for this unit yet.
                        </div>
                      ) : (
                        <div className="max-h-[26rem] space-y-2 overflow-y-auto pr-1">
                          {selectedMinecraftServerLogs.map((line, index) => (
                            <div
                              key={`${line.ts_ms ?? 'no-ts'}-${index}`}
                              className="rounded-xl border border-[var(--border)] bg-[var(--panel)]/55 px-4 py-3 text-sm"
                            >
                              <div className="text-white">{line.message}</div>
                              <div className="mt-2 text-xs muted">
                                {formatTsMs(line.ts_ms)}
                                {line.priority ? ` · priority ${line.priority}` : ''}
                              </div>
                            </div>
                          ))}
                        </div>
                      )}
                    </div>
                  </section>
                </>
              ) : (
                <div className="panel-soft rounded-xl px-4 py-3 text-sm muted">
                  Choose a server from the list to inspect its diagnostics.
                </div>
              )}
            </div>
          </section>
        </div>
      )}

      {activeTab === 'logs' && (
        <section className="space-y-4">
          <div className="panel-soft rounded-2xl p-5">
            <div className="flex items-start justify-between gap-4">
              <div>
                <h2 className="text-xl font-semibold">Runtime Diagnostics</h2>
                <p className="text-sm muted">
                  Lightweight live counters for long-running Rustyfin health, websocket usage, transcoding, jobs, and internal agent calls.
                </p>
              </div>
              {runtimeDiagnosticsLoading ? <span className="chip text-[11px]">Refreshing</span> : null}
            </div>
            {runtimeDiagnostics ? (
              <div className="mt-4 grid gap-3 xl:grid-cols-4">
                <div className="rounded-2xl border border-[var(--border)] bg-[var(--panel)]/45 p-4 text-sm">
                  <div className="text-xs font-semibold uppercase tracking-[0.18em] text-white/85">Host</div>
                  <div className="mt-3 space-y-2">
                    {runtimeDiagnostics.host.available ? (
                      <>
                        <div className="flex items-center justify-between gap-3">
                          <span className="muted">Host uptime</span>
                          <span>
                            {runtimeDiagnostics.host.uptime_seconds != null
                              ? formatUptime(runtimeDiagnostics.host.uptime_seconds)
                              : '—'}
                          </span>
                        </div>
                        <div className="flex items-center justify-between gap-3">
                          <span className="muted">CPU</span>
                          <span>
                            {formatPercent(runtimeDiagnostics.host.cpu_usage_percent)} /{' '}
                            {runtimeDiagnostics.host.logical_cpu_threads ?? '—'} threads
                          </span>
                        </div>
                        <div className="flex items-center justify-between gap-3">
                          <span className="muted">Memory</span>
                          <span>
                            {formatBytes(runtimeDiagnostics.host.used_memory_bytes)} /{' '}
                            {formatBytes(runtimeDiagnostics.host.total_memory_bytes)}
                          </span>
                        </div>
                        <div className="flex items-center justify-between gap-3">
                          <span className="muted">Load</span>
                          <span>
                            {runtimeDiagnostics.host.load_average
                              ? `${runtimeDiagnostics.host.load_average.one.toFixed(1)} / ${runtimeDiagnostics.host.load_average.five.toFixed(1)} / ${runtimeDiagnostics.host.load_average.fifteen.toFixed(1)}`
                              : '—'}
                          </span>
                        </div>
                      </>
                    ) : (
                      <div className="text-sm muted">
                        {runtimeDiagnostics.host.reason || 'Host runtime stats are unavailable on this host.'}
                      </div>
                    )}
                  </div>
                </div>

                <div className="rounded-2xl border border-[var(--border)] bg-[var(--panel)]/45 p-4 text-sm">
                  <div className="text-xs font-semibold uppercase tracking-[0.18em] text-white/85">Runtime</div>
                  <div className="mt-3 space-y-2">
                    <div className="flex items-center justify-between gap-3">
                      <span className="muted">Uptime</span>
                      <span>{formatUptime(runtimeDiagnostics.runtime.uptime_seconds)}</span>
                    </div>
                    <div className="flex items-center justify-between gap-3">
                      <span className="muted">Active jobs</span>
                      <span>{runtimeDiagnostics.runtime.jobs.total.active_running}</span>
                    </div>
                    <div className="flex items-center justify-between gap-3">
                      <span className="muted">Job failures</span>
                      <DiagnosticsTrend
                        lastMinute={runtimeDiagnostics.runtime.jobs.total.failures_last_minute}
                        lastFiveMinutes={
                          runtimeDiagnostics.runtime.jobs.total.failures_last_five_minutes
                        }
                      />
                    </div>
                  </div>
                </div>

                <div className="rounded-2xl border border-[var(--border)] bg-[var(--panel)]/45 p-4 text-sm">
                  <div className="text-xs font-semibold uppercase tracking-[0.18em] text-white/85">Transcoding</div>
                  <div className="mt-3 space-y-2">
                    <div className="flex items-center justify-between gap-3">
                      <span className="muted">Active sessions</span>
                      <span>{runtimeDiagnostics.transcoding.active_sessions}</span>
                    </div>
                    <div className="flex items-center justify-between gap-3">
                      <span className="muted">Created</span>
                      <span>{runtimeDiagnostics.transcoding.created_total}</span>
                    </div>
                    <div className="flex items-center justify-between gap-3">
                      <span className="muted">Create failures</span>
                      <DiagnosticsTrend
                        lastMinute={runtimeDiagnostics.transcoding.create_failures_last_minute}
                        lastFiveMinutes={
                          runtimeDiagnostics.transcoding.create_failures_last_five_minutes
                        }
                      />
                    </div>
                  </div>
                </div>

                <div className="rounded-2xl border border-[var(--border)] bg-[var(--panel)]/45 p-4 text-sm">
                  <div className="text-xs font-semibold uppercase tracking-[0.18em] text-white/85">WebSockets</div>
                  <div className="mt-3 space-y-2">
                    <div className="flex items-center justify-between gap-3">
                      <span className="muted">Channels</span>
                      <span>{runtimeDiagnostics.runtime.websockets.channels.active} active / {runtimeDiagnostics.runtime.websockets.channels.connections_total} total</span>
                    </div>
                    <div className="flex items-center justify-between gap-3">
                      <span className="muted">Rooms</span>
                      <span>{runtimeDiagnostics.runtime.websockets.watch_party.active} active / {runtimeDiagnostics.runtime.websockets.watch_party.connections_total} total</span>
                    </div>
                  </div>
                </div>

                <div className="rounded-2xl border border-[var(--border)] bg-[var(--panel)]/45 p-4 text-sm">
                  <div className="text-xs font-semibold uppercase tracking-[0.18em] text-white/85">Agent Calls</div>
                  <div className="mt-3 space-y-2">
                    {[
                      { label: 'Servers', agent: runtimeDiagnostics.runtime.agents.servers },
                      { label: 'TMDB', agent: runtimeDiagnostics.runtime.agents.tmdb },
                      {
                        label: 'Transcription',
                        agent: runtimeDiagnostics.runtime.agents.transcription,
                      },
                      {
                        label: 'YouTube Music',
                        agent: runtimeDiagnostics.runtime.agents.youtube,
                      },
                    ].map(({ label, agent }) => (
                      <div key={label} className="flex items-center justify-between gap-3">
                        <span className="muted">{label}</span>
                        <span className="flex items-center gap-3">
                          <span>{agent.calls_in_flight} in flight</span>
                          <DiagnosticsTrend
                            lastMinute={agent.failures_last_minute}
                            lastFiveMinutes={agent.failures_last_five_minutes}
                          />
                        </span>
                      </div>
                    ))}
                  </div>
                </div>

                <div className="rounded-2xl border border-[var(--border)] bg-[var(--panel)]/45 p-4 text-sm">
                  <div className="text-xs font-semibold uppercase tracking-[0.18em] text-white/85">AI Assistant</div>
                  <div className="mt-3 space-y-2">
                    <div className="flex items-center justify-between gap-3">
                      <span className="muted">Chats</span>
                      <span className="flex items-center gap-3">
                        <span>{runtimeDiagnostics.runtime.assistant.chats.calls_in_flight} in flight / {runtimeDiagnostics.runtime.assistant.chats.calls_total} total</span>
                        <DiagnosticsTrend
                          lastMinute={runtimeDiagnostics.runtime.assistant.chats.failures_last_minute}
                          lastFiveMinutes={runtimeDiagnostics.runtime.assistant.chats.failures_last_five_minutes}
                        />
                      </span>
                    </div>
                    <div className="flex items-center justify-between gap-3">
                      <span className="muted">Grounded tools</span>
                      <span className="flex items-center gap-3">
                        <span>{runtimeDiagnostics.runtime.assistant.tools.calls_in_flight} in flight / {runtimeDiagnostics.runtime.assistant.tools.calls_total} total</span>
                        <DiagnosticsTrend
                          lastMinute={runtimeDiagnostics.runtime.assistant.tools.failures_last_minute}
                          lastFiveMinutes={runtimeDiagnostics.runtime.assistant.tools.failures_last_five_minutes}
                        />
                      </span>
                    </div>
                  </div>
                </div>
              </div>
            ) : (
              <div className="mt-4 rounded-2xl border border-[var(--border)] bg-[var(--panel)]/45 px-4 py-3 text-sm muted">
                Runtime diagnostics are not available yet.
              </div>
            )}
            <div className="mt-3 text-xs muted">
              Failure windows are shown as <span className="text-white">last 1 minute / last 5 minutes</span>.
            </div>
          </div>

          <section className="space-y-3">
            <h2 className="text-xl font-semibold">Logs</h2>
          <div className="flex flex-wrap gap-2 border-b border-[var(--border)] pb-0">
            {LOG_FILTER_TABS.map((tab) => (
              <button
                key={tab.key}
                type="button"
                onClick={() => setLogFilterTab(tab.key)}
                className={`px-5 py-2.5 text-sm font-medium rounded-t-lg transition-colors ${
                  logFilterTab === tab.key
                    ? 'bg-[var(--surface)] border border-b-0 border-[var(--border)]'
                    : 'opacity-60 hover:opacity-100 hover:bg-[var(--surface)] hover:bg-opacity-50 hover:border hover:border-b-0 hover:border-[var(--border)] hover:border-opacity-50'
                }`}
              >
                {tab.label}
              </button>
            ))}
          </div>
          {filteredLogJobs.length === 0 ? (
            <p className="text-sm muted">No logs for this filter</p>
          ) : (
            filteredLogJobs.map((job) => {
              const isTerminal = !['queued', 'running'].includes(job.status);
              return (
                <div key={job.id} data-admin-job-id={job.id} className="tile space-y-2 p-3">
                  <div className="flex items-center justify-between gap-3">
                    <div>
                      <p className="text-sm font-medium">{job.kind}</p>
                      <p className="text-xs muted">
                        {new Date(job.created_ts * 1000).toLocaleString()}
                      </p>
                    </div>
                    <div className="flex items-center gap-2">
                      <span className="chip">{job.status}</span>
                      {isTerminal && (
                        <button
                          onClick={() => deleteJob(job.id)}
                          className="btn-ghost px-2 py-1 text-xs text-[var(--danger)]"
                          title="Delete log"
                        >
                          Delete
                        </button>
                      )}
                    </div>
                  </div>
                  <p className="text-xs muted">{Math.round(job.progress * 100)}%</p>
                  <div className="h-2 overflow-hidden rounded-full bg-white/8">
                    <div
                      className="h-full rounded-full bg-gradient-to-r from-[var(--orange)] to-[var(--purple)]"
                      style={{
                        width: `${Math.max(0, Math.min(100, Math.round(job.progress * 100)))}%`,
                      }}
                    />
                  </div>
                  {job.payload && (
                    <pre className="max-h-40 overflow-auto rounded-lg bg-black/20 px-2 py-1 text-xs muted">
                      {JSON.stringify(job.payload, null, 2)}
                    </pre>
                  )}
                  {job.error && (
                    <p className="text-xs text-red-300">{job.error}</p>
                  )}
                </div>
              );
            })
          )}
          </section>
        </section>
      )}

      {activeTab === 'vault_audit' && (
        <section className="panel space-y-5 p-6">
          <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
            <div>
              <h2 className="text-xl font-semibold">Vault Audit</h2>
              <p className="text-sm muted">
                Read-only RustyVault audit history for the current admin account. This uses the same RustyVault web-session boundary as the Vault page.
              </p>
            </div>
            <button
              type="button"
              onClick={() => void loadVaultAudit()}
              className="btn-secondary px-4 py-2 text-sm"
            >
              Refresh audit
            </button>
          </div>

          {vaultAuditError ? (
            <div className="notice-error rounded-xl px-4 py-3 text-sm">{vaultAuditError}</div>
          ) : null}

          {vaultAuditLoading ? (
            <div className="panel-soft rounded-xl px-4 py-3 text-sm muted">
              Loading Vault audit…
            </div>
          ) : vaultAuditEvents.length === 0 ? (
            <div className="panel-soft rounded-xl px-4 py-3 text-sm muted">
              No vault audit events are available for this account yet.
            </div>
          ) : (
            <div className="space-y-3">
              {vaultAuditEvents.map((event) => (
                <div key={event.id} className="tile space-y-3 px-4 py-4">
                  <div className="flex flex-wrap items-start justify-between gap-3">
                    <div>
                      <p className="font-medium">{titleCase(event.event_kind)}</p>
                      <p className="mt-1 text-xs muted">{formatTs(event.created_ts)}</p>
                    </div>
                    <span className="chip">{event.target_item_id || 'account scope'}</span>
                  </div>
                  <pre className="overflow-x-auto rounded-xl bg-black/20 px-3 py-3 text-xs muted">
                    {JSON.stringify(event.event_json, null, 2)}
                  </pre>
                </div>
              ))}
            </div>
          )}
        </section>
      )}

      {activeTab === 'tmdb' && (
        <section className="panel space-y-4 p-6">
          <h2 className="text-xl font-semibold">TMDB Metadata</h2>
          <p className="text-sm muted">
            Set a TMDB API key so scans can fetch posters and metadata for detected movies/shows.
          </p>
          <form onSubmit={saveTmdbKey} className="space-y-3">
            <input
              type="password"
              value={tmdbApiKey}
              onChange={(e) => setTmdbApiKey(e.target.value)}
              placeholder="Enter TMDB API key (leave empty to clear)"
              className="input w-full px-3 py-2 text-sm"
            />
            <div className="flex flex-wrap items-center gap-2">
              <button
                type="submit"
                disabled={savingTmdb}
                className="btn-primary px-4 py-2 text-sm disabled:opacity-50"
              >
                {savingTmdb ? 'Saving...' : 'Save TMDB Key'}
              </button>
              <button
                type="button"
                onClick={clearTmdbKey}
                disabled={savingTmdb}
                className="btn-secondary px-4 py-2 text-sm disabled:opacity-50"
              >
                Clear Stored Key
              </button>
            </div>
          </form>
          <p className="text-xs muted">
            Status:{' '}
            {tmdbConfig.configured
              ? `configured (${tmdbConfig.source || 'unknown'}${
                  tmdbConfig.key_preview ? `, ${tmdbConfig.key_preview}` : ''
                })`
              : 'not configured'}
          </p>

          <div className="panel-soft space-y-3 rounded-xl p-4">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <h3 className="text-sm font-semibold">TMDB Sync Status</h3>
              <p className="text-xs muted">Scheduler checks every 60 seconds</p>
            </div>
            {tmdbSyncStatusRows.length === 0 ? (
              <p className="text-xs muted">No movie/TV libraries configured for TMDB sync.</p>
            ) : (
              <div className="overflow-x-auto">
                <table className="min-w-[760px] w-full text-sm">
                  <thead>
                    <tr className="border-b border-[var(--border)] text-left text-xs uppercase tracking-[0.14em] muted">
                      <th className="px-2 py-2 font-medium">Library</th>
                      <th className="px-2 py-2 font-medium">Last Run Result</th>
                      <th className="px-2 py-2 font-medium">Next Scheduled Run</th>
                      <th className="px-2 py-2 font-medium">Failure Reason</th>
                    </tr>
                  </thead>
                  <tbody>
                    {tmdbSyncStatusRows.map((row) => (
                      <tr key={row.library_id} className="border-b border-[var(--border)]/60">
                        <td className="px-2 py-2 align-top">
                          <p className="font-medium">{row.library_name}</p>
                          <p className="text-xs muted">{row.library_kind}</p>
                        </td>
                        <td className="px-2 py-2 align-top">
                          <p>{row.last_run_result}</p>
                          <p className="text-xs muted">{formatTs(row.last_run_ts)}</p>
                        </td>
                        <td className="px-2 py-2 align-top">
                          <p>{row.next_scheduled_run_label}</p>
                        </td>
                        <td className="px-2 py-2 align-top">
                          {row.failure_reason ? (
                            <p className="text-xs text-red-300" title={row.failure_reason}>
                              {row.failure_reason}
                            </p>
                          ) : (
                            <p className="text-xs muted">—</p>
                          )}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
          </div>
        </section>
      )}

      {hostDirBrowserOpen && (
        <div className="fixed inset-0 z-40 flex items-center justify-center bg-black/45 backdrop-blur-[2px] p-4">
          <div className="panel w-full max-w-3xl max-h-[82vh] rounded-2xl border border-[var(--border)] p-4 sm:p-5 flex flex-col gap-3">
            <div className="flex items-center justify-between gap-2">
              <div>
                <h2 className="text-lg font-semibold">Browse Backend Directories</h2>
                <p className="text-xs muted">
                  Selecting folders from the server filesystem (Debian host or mounted media
                  roots).
                </p>
              </div>
              <button
                type="button"
                onClick={closeHostDirectoryBrowser}
                className="btn-ghost px-3 py-1.5 text-sm"
              >
                Close
              </button>
            </div>

            {hostDirBrowserRoots.length > 0 && (
              <div className="flex flex-wrap gap-2">
                {hostDirBrowserRoots.map((rootPath) => (
                  <button
                    key={rootPath}
                    type="button"
                    onClick={() => navigateHostDirectory(rootPath)}
                    className={`btn-ghost px-2.5 py-1 text-xs ${
                      hostDirBrowserCurrentPath.startsWith(rootPath)
                        ? 'border-[var(--orange-soft)] text-[var(--orange-soft)]'
                        : ''
                    }`}
                  >
                    {rootPath}
                  </button>
                ))}
              </div>
            )}

            <div className="panel-soft rounded-xl border border-[var(--border)] px-3 py-2 flex items-center gap-2">
              <button
                type="button"
                onClick={() => navigateHostDirectory(hostDirBrowserParentPath)}
                disabled={!hostDirBrowserParentPath || hostDirBrowserLoading}
                className="btn-secondary px-3 py-1 text-xs disabled:opacity-50"
              >
                Up
              </button>
              <div className="min-w-0">
                <p className="text-[11px] uppercase tracking-[0.12em] muted">Current Path</p>
                <p className="text-sm font-mono truncate" title={hostDirBrowserCurrentPath}>
                  {hostDirBrowserCurrentPath || '—'}
                </p>
              </div>
            </div>

            {hostDirBrowserError && <p className="text-sm text-red-300">{hostDirBrowserError}</p>}

            <div className="panel-soft min-h-[260px] overflow-auto rounded-xl border border-[var(--border)] p-2">
              {hostDirBrowserLoading ? (
                <p className="px-2 py-2 text-sm muted">Loading directories…</p>
              ) : hostDirBrowserDirectories.length === 0 ? (
                <p className="px-2 py-2 text-sm muted">No child directories found.</p>
              ) : (
                <div className="space-y-1">
                  {hostDirBrowserDirectories.map((entry) => (
                    <button
                      key={entry.path}
                      type="button"
                      onClick={() => navigateHostDirectory(entry.path)}
                      className="w-full rounded-lg border border-[var(--border)] bg-[var(--panel)]/65 px-3 py-2 text-left text-sm hover:border-[var(--orange-soft)]/55 hover:bg-[var(--panel)]"
                      title={entry.path}
                    >
                      <div className="font-medium">{entry.name}</div>
                      <div className="truncate text-xs muted">{entry.path}</div>
                    </button>
                  ))}
                </div>
              )}
            </div>

            <div className="flex items-center justify-end gap-2">
              <button
                type="button"
                onClick={closeHostDirectoryBrowser}
                className="btn-ghost px-4 py-2 text-sm"
              >
                Cancel
              </button>
              <button
                type="button"
                onClick={confirmHostDirectorySelection}
                disabled={hostDirBrowserLoading || !hostDirBrowserCurrentPath}
                className="btn-primary px-4 py-2 text-sm disabled:opacity-50"
              >
                Use This Folder
              </button>
            </div>
          </div>
        </div>
      )}

      <ConfirmModal
        open={Boolean(pendingDeleteAction)}
        title="Confirm Delete"
        description={
          pendingDeleteAction
            ? `Delete ${pendingDeleteAction.label}? This action cannot be undone.`
            : undefined
        }
        confirmLabel="Delete"
        destructive
        onCancel={() => setPendingDeleteAction(null)}
        onConfirm={() => {
          void confirmPendingDelete();
        }}
      />

      <ConfirmModal
        open={Boolean(pendingRoomEndAction)}
        title="End Room"
        description={
          pendingRoomEndAction
            ? `End room ${pendingRoomEndAction.label}? All participants will be disconnected.`
            : undefined
        }
        confirmLabel="End room"
        destructive
        onCancel={() => setPendingRoomEndAction(null)}
        onConfirm={() => {
          if (!pendingRoomEndAction) return;
          void endRoom(pendingRoomEndAction.id);
        }}
      />
    </div>
  );
}

function defaultMusicImportState(): MusicImportState {
  return {
    source: '',
    artist: '',
    album: 'Singles',
    title: '',
    importing: false,
  };
}

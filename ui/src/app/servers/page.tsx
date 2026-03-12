'use client';

import { useEffect, useRef, useState } from 'react';
import { useRouter } from 'next/navigation';
import ConfirmModal from '@/app/components/ConfirmModal';

import { useAuth } from '@/lib/auth';
import { clientErrorMessage } from '@/lib/errors';
import {
  deleteMinecraftServer,
  MinecraftServerAction,
  MinecraftRuntimeCapabilities,
  MinecraftServer,
  MinecraftServerActionResponse,
  MinecraftServerOperationResponse,
  createMinecraftServer,
  getMinecraftRuntimeCapabilities,
  HostDirectoryListEntry,
  listBackendHostDirectories,
  listMinecraftServers,
  importMinecraftServer,
  refreshMinecraftServerStatus,
  requestMinecraftServerAction,
  updateMinecraftServer,
} from '@/lib/serversApi';

type CreateFormState = {
  display_name: string;
  description: string;
  server_distribution: 'vanilla' | 'paper';
  minecraft_version: string;
  world_name: string;
  listen_port: string;
  gamemode: 'survival' | 'creative' | 'adventure' | 'spectator';
  difficulty: 'peaceful' | 'easy' | 'normal' | 'hard';
  hardcore: boolean;
  motd: string;
  max_player_count: string;
  min_memory_mb: string;
  max_memory_mb: string;
  online_mode: boolean;
  pvp: boolean;
  allow_flight: boolean;
  enable_command_block: boolean;
  white_list_enabled: boolean;
  autostart: boolean;
  eula_accepted: boolean;
};

type HostDirectoryBrowserState = {
  open: boolean;
  loading: boolean;
  error: string;
  currentPath: string;
  parentPath: string | null;
  roots: string[];
  directories: HostDirectoryListEntry[];
};

type CreateWizardMode = 'create' | 'import';
type CreateWizardStepId = 'mode' | 'core' | 'gameplay' | 'resources' | 'source' | 'review';

type ServerSettingsFormState = {
  gamemode: 'survival' | 'creative' | 'adventure' | 'spectator';
  difficulty: 'peaceful' | 'easy' | 'normal' | 'hard';
  hardcore: boolean;
  motd: string;
  max_player_count: string;
  autostart: boolean;
  online_mode: boolean;
  pvp: boolean;
  allow_flight: boolean;
  enable_command_block: boolean;
  white_list_enabled: boolean;
};

type ServersGameTab = 'minecraft' | 'more-soon';

const DEFAULT_FORM: CreateFormState = {
  display_name: '',
  description: '',
  server_distribution: 'paper',
  minecraft_version: '1.21.1',
  world_name: '',
  listen_port: '25565',
  gamemode: 'survival',
  difficulty: 'normal',
  hardcore: false,
  motd: '',
  max_player_count: '20',
  min_memory_mb: '1024',
  max_memory_mb: '4096',
  online_mode: true,
  pvp: true,
  allow_flight: false,
  enable_command_block: false,
  white_list_enabled: false,
  autostart: false,
  eula_accepted: false,
};

const CREATE_WIZARD_STEP_META: Record<
  CreateWizardStepId,
  {
    title: string;
    description: string;
  }
> = {
  mode: {
    title: 'Server type',
    description: 'Choose whether Rustyfin should build a new managed server or import an existing one.',
  },
  core: {
    title: 'Server basics',
    description: 'Choose the distribution, version, server identity, and connection port.',
  },
  gameplay: {
    title: 'Gameplay',
    description: 'Set the world behavior players will see when they first join.',
  },
  resources: {
    title: 'Resources',
    description: 'Size the server for memory and player capacity.',
  },
  source: {
    title: 'Import source',
    description: 'Point Rustyfin at the existing Minecraft server directory on the Debian host.',
  },
  review: {
    title: 'Rules and review',
    description: 'Confirm server rules, review the setup, and accept the EULA.',
  },
};

function createWizardSteps(mode: CreateWizardMode | null) {
  const base: CreateWizardStepId[] = ['mode', 'core', 'gameplay', 'resources'];
  if (mode === 'import') {
    base.push('source');
  }
  base.push('review');
  return base;
}

const TOGGLE_FIELDS: Array<{ key: keyof Pick<
  CreateFormState,
  | 'online_mode'
  | 'allow_flight'
  | 'enable_command_block'
  | 'white_list_enabled'
  | 'autostart'
>; label: string }> = [
  { key: 'online_mode', label: 'Online mode' },
  { key: 'allow_flight', label: 'Allow flight' },
  { key: 'enable_command_block', label: 'Enable command blocks' },
  { key: 'white_list_enabled', label: 'Enable whitelist' },
  { key: 'autostart', label: 'Autostart on host boot' },
];

const SERVER_SETTINGS_TOGGLE_FIELDS: Array<{
  key: keyof Pick<
    ServerSettingsFormState,
    | 'online_mode'
    | 'pvp'
    | 'allow_flight'
    | 'enable_command_block'
    | 'white_list_enabled'
    | 'autostart'
    | 'hardcore'
  >;
  label: string;
}> = [
  { key: 'online_mode', label: 'Online mode' },
  { key: 'pvp', label: 'PVP enabled' },
  { key: 'allow_flight', label: 'Allow flight' },
  { key: 'enable_command_block', label: 'Enable command blocks' },
  { key: 'white_list_enabled', label: 'Enable whitelist' },
  { key: 'autostart', label: 'Autostart on host boot' },
  { key: 'hardcore', label: 'Hardcore mode' },
];

function createStepErrors(
  form: CreateFormState,
  step: CreateWizardStepId,
  mode: CreateWizardMode | null,
  importSourcePath: string,
) {
  switch (step) {
    case 'mode':
      return mode ? [] : ['Choose whether this should be a new managed server or an imported server.'];
    case 'core': {
      const errors: string[] = [];
      if (!form.display_name.trim()) {
        errors.push('Display name is required.');
      }
      if (!form.minecraft_version.trim()) {
        errors.push('Minecraft version is required.');
      }
      if (!form.world_name.trim()) {
        errors.push('World name is required.');
      }
      const port = Number(form.listen_port);
      if (!Number.isInteger(port) || port < 1 || port > 65535) {
        errors.push('Port must be a whole number between 1 and 65535.');
      }
      return errors;
    }
    case 'source':
      return importSourcePath.trim() ? [] : ['Host source path is required for an imported server.'];
    case 'resources': {
      const errors: string[] = [];
      const minMemory = Number(form.min_memory_mb);
      const maxMemory = Number(form.max_memory_mb);
      const maxPlayers = Number(form.max_player_count);
      if (!Number.isInteger(minMemory) || minMemory < 512) {
        errors.push('Minimum RAM must be at least 512 MB.');
      }
      if (!Number.isInteger(maxMemory) || maxMemory < 512) {
        errors.push('Maximum RAM must be at least 512 MB.');
      }
      if (
        Number.isInteger(minMemory) &&
        Number.isInteger(maxMemory) &&
        minMemory >= 512 &&
        maxMemory >= 512 &&
        maxMemory < minMemory
      ) {
        errors.push('Maximum RAM must be greater than or equal to minimum RAM.');
      }
      if (!Number.isInteger(maxPlayers) || maxPlayers < 1) {
        errors.push('Max players must be at least 1.');
      }
      return errors;
    }
    case 'review':
      return form.eula_accepted ? [] : ['You must confirm the Minecraft EULA before creating the server.'];
    default:
      return [];
  }
}

function formatTs(ts?: number | null) {
  if (!ts) return 'Never';
  return new Date(ts * 1000).toLocaleString();
}

function titleCase(value: string) {
  return value
    .split(/[_\s-]+/)
    .filter(Boolean)
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(' ');
}

function serverToSettingsForm(server: MinecraftServer): ServerSettingsFormState {
  return {
    gamemode: server.gamemode as ServerSettingsFormState['gamemode'],
    difficulty: server.difficulty as ServerSettingsFormState['difficulty'],
    hardcore: server.hardcore,
    motd: server.motd,
    max_player_count: String(server.max_player_count ?? 20),
    autostart: server.autostart,
    online_mode: server.online_mode,
    pvp: server.pvp,
    allow_flight: server.allow_flight,
    enable_command_block: server.enable_command_block,
    white_list_enabled: server.white_list_enabled,
  };
}

function getServerIndicator(server: MinecraftServer) {
  if (server.observed_state === 'draft') {
    return {
      label: 'Draft',
      dotClass: 'bg-slate-300 shadow-[0_0_14px_rgba(226,232,240,0.22)]',
      textClass: 'text-slate-200',
      animated: false,
    };
  }

  if (server.observed_state === 'unprovisioned') {
    return {
      label: 'Needs Provisioning',
      dotClass: 'bg-amber-400 shadow-[0_0_14px_rgba(251,191,36,0.4)]',
      textClass: 'text-amber-200',
      animated: false,
    };
  }

  if (server.observed_state === 'provisioning' || server.observed_state === 'importing') {
    return {
      label: titleCase(server.observed_state),
      dotClass: 'bg-amber-400 shadow-[0_0_14px_rgba(251,191,36,0.45)]',
      textClass: 'text-amber-200',
      animated: true,
    };
  }

  if (server.observed_state === 'running' && server.health_state === 'pending') {
    return {
      label: 'Booting',
      dotClass: 'bg-amber-400 shadow-[0_0_14px_rgba(251,191,36,0.45)]',
      textClass: 'text-amber-200',
      animated: true,
    };
  }

  if (server.observed_state === 'running' && server.health_state === 'healthy') {
    return {
      label: 'Online',
      dotClass: 'bg-emerald-400 shadow-[0_0_14px_rgba(74,222,128,0.45)]',
      textClass: 'text-emerald-200',
      animated: false,
    };
  }

  if (server.observed_state === 'starting' || server.observed_state === 'restarting') {
    return {
      label: titleCase(server.observed_state),
      dotClass: 'bg-amber-400 shadow-[0_0_14px_rgba(251,191,36,0.45)]',
      textClass: 'text-amber-200',
      animated: true,
    };
  }

  if (server.observed_state === 'stopping') {
    return {
      label: 'Stopping',
      dotClass: 'bg-amber-400 shadow-[0_0_14px_rgba(251,191,36,0.45)]',
      textClass: 'text-amber-200',
      animated: true,
    };
  }

  if (server.health_state === 'error' || server.observed_state === 'error') {
    return {
      label: 'Error',
      dotClass: 'bg-rose-400 shadow-[0_0_14px_rgba(251,113,133,0.4)]',
      textClass: 'text-rose-200',
      animated: false,
    };
  }

  return {
    label: 'Offline',
    dotClass: 'bg-rose-400 shadow-[0_0_14px_rgba(251,113,133,0.4)]',
    textClass: 'text-rose-200',
    animated: false,
  };
}

function getServerProgressMessage(server: MinecraftServer) {
  if (server.observed_state === 'draft') {
    return 'Created. Click Start to provision the server files and launch the Minecraft service.';
  }

  if (server.observed_state === 'unprovisioned') {
    return 'Ready to provision. Click Start to create the native service and first-time server files.';
  }

  if (
    (server.health_state === 'error' || server.observed_state === 'failed' || server.observed_state === 'error') &&
    server.last_error_summary
  ) {
    return server.last_error_summary;
  }

  return null;
}

function shouldAutoRefreshServer(server: MinecraftServer) {
  return (
    server.observed_state === 'provisioning' ||
    server.observed_state === 'importing' ||
    server.observed_state === 'starting' ||
    server.observed_state === 'restarting' ||
    (server.observed_state === 'running' && server.health_state === 'pending')
  );
}

export default function ServersPage() {
  const router = useRouter();
  const { me, loading: authLoading } = useAuth();
  const serversRef = useRef<MinecraftServer[]>([]);

  const activeGameTab: ServersGameTab = 'minecraft';
  const [servers, setServers] = useState<MinecraftServer[]>([]);
  const [selectedServerId, setSelectedServerId] = useState<string | null>(null);
  const [serverEdits, setServerEdits] = useState<Record<string, ServerSettingsFormState>>({});
  const [loading, setLoading] = useState(true);
  const [statusRefreshingServerId, setStatusRefreshingServerId] = useState<string | null>(null);
  const [actionLoading, setActionLoading] = useState<MinecraftServerAction | null>(null);
  const [actionServerId, setActionServerId] = useState<string | null>(null);
  const [savingServerId, setSavingServerId] = useState<string | null>(null);
  const [importing, setImporting] = useState(false);
  const [creating, setCreating] = useState(false);
  const [deletingServerId, setDeletingServerId] = useState<string | null>(null);
  const [error, setError] = useState('');
  const [deleteConfirmServer, setDeleteConfirmServer] = useState<MinecraftServer | null>(null);
  const [runtimeCapabilities, setRuntimeCapabilities] = useState<MinecraftRuntimeCapabilities | null>(null);
  const [form, setForm] = useState<CreateFormState>(DEFAULT_FORM);
  const [createMode, setCreateMode] = useState<CreateWizardMode | null>(null);
  const [createStep, setCreateStep] = useState<CreateWizardStepId>('mode');
  const [importSourcePath, setImportSourcePath] = useState('');
  const [pendingImportServerId, setPendingImportServerId] = useState<string | null>(null);
  const [hostBrowser, setHostBrowser] = useState<HostDirectoryBrowserState>({
    open: false,
    loading: false,
    error: '',
    currentPath: '',
    parentPath: null,
    roots: [],
    directories: [],
  });

  useEffect(() => {
    serversRef.current = servers;
  }, [servers]);

  useEffect(() => {
    if (!authLoading && !me) {
      router.replace('/login');
    }
  }, [authLoading, me, router]);

  useEffect(() => {
    if (!me) return;

    let cancelled = false;
    setLoading(true);
    setError('');

    (async () => {
      try {
        const [rows, capabilities] = await Promise.all([
          listMinecraftServers(),
          getMinecraftRuntimeCapabilities(),
        ]);
        if (cancelled) return;
        setServers(rows);
        setRuntimeCapabilities(capabilities);
        setServerEdits((prev) => {
          const next: Record<string, ServerSettingsFormState> = {};
          for (const row of rows) {
            next[row.id] = prev[row.id] ?? serverToSettingsForm(row);
          }
          return next;
        });
        setSelectedServerId((current) => {
          if (current && rows.some((row) => row.id === current)) {
            return current;
          }
          return null;
        });
        if (capabilities.status_supported && rows.length > 0) {
          void refreshAllServerStatuses(rows);
        }
      } catch (err: unknown) {
        if (!cancelled) {
          setError(clientErrorMessage(err, 'Failed to load servers'));
        }
      } finally {
        if (!cancelled) {
          setLoading(false);
        }
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [me]);

  useEffect(() => {
    if (!me || !(runtimeCapabilities?.status_supported ?? true)) {
      return;
    }

    if (servers.length === 0) {
      return;
    }

    let cancelled = false;
    let inFlight = false;
    const hasActiveTransitions = servers.some((server) => shouldAutoRefreshServer(server));
    const refreshIntervalMs = hasActiveTransitions ? 3000 : 15000;

    const refreshAllVisibleServers = async () => {
      if (cancelled || inFlight) return;
      if (typeof document !== 'undefined' && document.visibilityState !== 'visible') {
        return;
      }
      inFlight = true;
      try {
        await refreshAllServerStatuses(servers);
      } finally {
        inFlight = false;
      }
    };

    void refreshAllVisibleServers();
    const interval = window.setInterval(() => {
      void refreshAllVisibleServers();
    }, refreshIntervalMs);

    return () => {
      cancelled = true;
      window.clearInterval(interval);
    };
  }, [me, runtimeCapabilities?.status_supported, servers]);

  useEffect(() => {
    if (!me || !(runtimeCapabilities?.status_supported ?? true)) {
      return;
    }

    const handleVisibilityRefresh = () => {
      if (document.visibilityState !== 'visible') {
        return;
      }
      void refreshAllServerStatuses(serversRef.current);
    };

    const handleWindowFocus = () => {
      void refreshAllServerStatuses(serversRef.current);
    };

    document.addEventListener('visibilitychange', handleVisibilityRefresh);
    window.addEventListener('focus', handleWindowFocus);

    return () => {
      document.removeEventListener('visibilitychange', handleVisibilityRefresh);
      window.removeEventListener('focus', handleWindowFocus);
    };
  }, [me, runtimeCapabilities?.status_supported]);

  function updateForm<K extends keyof CreateFormState>(key: K, value: CreateFormState[K]) {
    setForm((prev) => ({ ...prev, [key]: value }));
  }

  function upsertServer(updated: MinecraftServer) {
    setServers((prev) => {
      const existingIndex = prev.findIndex((server) => server.id === updated.id);
      if (existingIndex === -1) {
        return [updated, ...prev];
      }
      const next = [...prev];
      next[existingIndex] = updated;
      return next;
    });
  }

  function closeHostDirectoryBrowser() {
    setHostBrowser((prev) => ({
      ...prev,
      open: false,
      loading: false,
      error: '',
    }));
  }

  async function fetchHostDirectories(path?: string) {
    const listing = await listBackendHostDirectories(path);
    setHostBrowser((prev) => ({
      ...prev,
      loading: false,
      error: '',
      currentPath: listing.current_path,
      parentPath: listing.parent_path ?? null,
      roots: listing.roots,
      directories: listing.directories,
    }));
  }

  function openHostDirectoryBrowser(initialPath?: string) {
    setHostBrowser((prev) => ({
      ...prev,
      open: true,
      loading: true,
      error: '',
    }));
    void fetchHostDirectories(initialPath)
      .catch((err: unknown) => {
        setHostBrowser((prev) => ({
          ...prev,
          loading: false,
          error: clientErrorMessage(err, 'Failed to browse backend directories'),
        }));
      });
  }

  function navigateHostDirectory(path?: string | null) {
    const target = path?.trim();
    if (!target) return;
    setHostBrowser((prev) => ({ ...prev, loading: true, error: '' }));
    void fetchHostDirectories(target).catch((err: unknown) => {
      setHostBrowser((prev) => ({
        ...prev,
        loading: false,
        error: clientErrorMessage(err, 'Failed to browse backend directories'),
      }));
    });
  }

  async function refreshServers(selectId?: string) {
    const rows = await listMinecraftServers();
    setServers(rows);
    setServerEdits((prev) => {
      const next: Record<string, ServerSettingsFormState> = {};
      for (const row of rows) {
        next[row.id] = prev[row.id] ?? serverToSettingsForm(row);
      }
      return next;
    });
    setSelectedServerId((current) => {
      if (current && rows.some((row) => row.id === current)) return current;
      return null;
    });
  }

  async function refreshSelectedServerStatus(serverId: string, showSpinner = true) {
    if (showSpinner) {
      setStatusRefreshingServerId(serverId);
    }
    try {
      const updated = await refreshMinecraftServerStatus(serverId);
      upsertServer(updated);
      return updated;
    } finally {
      if (showSpinner) {
        setStatusRefreshingServerId((current) => (current === serverId ? null : current));
      }
    }
  }

  async function refreshAllServerStatuses(serverRows: MinecraftServer[]) {
    if (!(runtimeCapabilities?.status_supported ?? true) || serverRows.length === 0) {
      return;
    }

    const refreshedRows = await Promise.all(
      serverRows.map((server) =>
        refreshMinecraftServerStatus(server.id).catch(() => null),
      ),
    );

    const refreshedById = new Map(
      refreshedRows
        .filter((row): row is MinecraftServer => row !== null)
        .map((row) => [row.id, row] as const),
    );

    if (refreshedById.size === 0) {
      return;
    }

    setServers((prev) =>
      prev.map((server) => refreshedById.get(server.id) ?? server),
    );
    setServerEdits((prev) => {
      const next = { ...prev };
      for (const updated of refreshedById.values()) {
        next[updated.id] = next[updated.id] ?? serverToSettingsForm(updated);
      }
      return next;
    });
  }

  async function handleRequestAction(server: MinecraftServer, action: MinecraftServerAction) {
    setActionServerId(server.id);
    setActionLoading(action);
    setError('');
    try {
      const response: MinecraftServerActionResponse = await requestMinecraftServerAction(server.id, action);
      upsertServer(response.instance);
      await refreshSelectedServerStatus(response.instance.id, false);
    } catch (err: unknown) {
      setError(clientErrorMessage(err, `Failed to ${action} server`));
    } finally {
      setActionLoading(null);
      setActionServerId((current) => (current === server.id ? null : current));
    }
  }

  function setServerEdit<K extends keyof ServerSettingsFormState>(
    serverId: string,
    key: K,
    value: ServerSettingsFormState[K],
  ) {
    const server = servers.find((entry) => entry.id === serverId);
    if (!server) {
      return;
    }
    setServerEdits((prev) => ({
      ...prev,
      [serverId]: {
        ...(prev[serverId] ?? serverToSettingsForm(server)),
        [key]: value,
      },
    }));
  }

  async function handleSaveServerSettings(server: MinecraftServer) {
    const edit = serverEdits[server.id] ?? serverToSettingsForm(server);
    setSavingServerId(server.id);
    setError('');
    try {
      const updated = await updateMinecraftServer(server.id, {
        gamemode: edit.gamemode,
        difficulty: edit.difficulty,
        hardcore: edit.hardcore,
        motd: edit.motd.trim(),
        max_player_count: Number(edit.max_player_count),
        autostart: edit.autostart,
        online_mode: edit.online_mode,
        pvp: edit.pvp,
        allow_flight: edit.allow_flight,
        enable_command_block: edit.enable_command_block,
        white_list_enabled: edit.white_list_enabled,
      });
      upsertServer(updated);
      setServerEdits((prev) => ({
        ...prev,
        [server.id]: serverToSettingsForm(updated),
      }));
    } catch (err: unknown) {
      setError(clientErrorMessage(err, 'Failed to update Minecraft server settings'));
    } finally {
      setSavingServerId((current) => (current === server.id ? null : current));
    }
  }

  async function handleCreateServer() {
    setCreating(true);
    setImporting(createMode === 'import');
    setError('');
    try {
      const created =
        pendingImportServerId !== null
          ? servers.find((server) => server.id === pendingImportServerId) ?? null
          : await createMinecraftServer({
              display_name: form.display_name,
              description: form.description,
              server_distribution: form.server_distribution,
              minecraft_version: form.minecraft_version,
              world_name: form.world_name,
              listen_port: Number(form.listen_port),
              gamemode: form.gamemode,
              difficulty: form.difficulty,
              hardcore: form.hardcore,
              motd: form.motd,
              max_player_count: Number(form.max_player_count),
              min_memory_mb: Number(form.min_memory_mb),
              max_memory_mb: Number(form.max_memory_mb),
              online_mode: form.online_mode,
              pvp: form.pvp,
              allow_flight: form.allow_flight,
              enable_command_block: form.enable_command_block,
              white_list_enabled: form.white_list_enabled,
              autostart: form.autostart,
              eula_accepted: form.eula_accepted,
            });

      if (!created) {
        throw new Error('Import target is no longer available. Restart the import wizard.');
      }

      if (createMode === 'import') {
        if (pendingImportServerId === null) {
          upsertServer(created);
          setPendingImportServerId(created.id);
        }
        const response: MinecraftServerOperationResponse = await importMinecraftServer(
          created.id,
          importSourcePath.trim(),
        );
        upsertServer(response.instance);
      } else {
        upsertServer(created);
      }

      setForm({
        ...DEFAULT_FORM,
        minecraft_version: form.minecraft_version,
        server_distribution: form.server_distribution,
      });
      setCreateMode(null);
      setCreateStep('mode');
      setImportSourcePath('');
      setPendingImportServerId(null);
      await refreshServers(created.id);
    } catch (err: unknown) {
      if (createMode === 'import') {
        setCreateStep('source');
      }
      const message =
        createMode === 'import'
          ? clientErrorMessage(err, 'Failed to import Minecraft server')
          : clientErrorMessage(err, 'Failed to create Minecraft server');
      setError(message);
    } finally {
      setCreating(false);
      setImporting(false);
    }
  }

  async function handleDeleteServer(server: MinecraftServer) {
    setDeletingServerId(server.id);
    setError('');
    try {
      await deleteMinecraftServer(server.id);
      setDeleteConfirmServer(null);
      await refreshServers();
    } catch (err: unknown) {
      setError(clientErrorMessage(err, 'Failed to delete Minecraft server'));
    } finally {
      setDeletingServerId(null);
    }
  }

  const createSteps = createWizardSteps(createMode);
  const createStepIndex = createSteps.findIndex((step) => step === createStep);
  const normalizedCreateStepIndex = createStepIndex === -1 ? 0 : createStepIndex;
  const activeCreateStepId = createSteps[normalizedCreateStepIndex] ?? createSteps[0];
  const activeCreateStep = CREATE_WIZARD_STEP_META[activeCreateStepId];
  const activeCreateStepErrors = createStepErrors(form, activeCreateStepId, createMode, importSourcePath);
  const canAdvanceCreateStep =
    activeCreateStepId === 'mode' ? createMode !== null : activeCreateStepErrors.length === 0;
  const createProgressPercent = ((normalizedCreateStepIndex + 1) / createSteps.length) * 100;

  function goToNextCreateStep() {
    if (!canAdvanceCreateStep) {
      return;
    }
    const nextStep = createSteps[normalizedCreateStepIndex + 1];
    if (nextStep) {
      setCreateStep(nextStep);
    }
  }

  function goToPreviousCreateStep() {
    const previousStep = createSteps[normalizedCreateStepIndex - 1];
    if (previousStep) {
      setCreateStep(previousStep);
    }
  }

  if (authLoading || !me) {
    return (
      <main className="mx-auto flex w-full max-w-7xl flex-col gap-6 px-4 py-6 sm:px-6 lg:px-8">
        <div className="panel-soft animate-rise px-5 py-4 text-sm muted">Loading servers…</div>
      </main>
    );
  }

  return (
    <main className="mx-auto flex w-full max-w-7xl flex-col gap-6 px-4 py-6 sm:px-6 lg:px-8">
      {error ? (
        <div className="panel-soft animate-rise border border-red-400/30 px-5 py-4 text-sm text-red-200">
          {error}
        </div>
      ) : null}

      {runtimeCapabilities?.reason ? (
        <div className="panel-soft animate-rise border border-amber-400/30 px-5 py-4 text-sm text-amber-100">
          {runtimeCapabilities.reason}
        </div>
      ) : null}

      <section className="panel relative mt-[17px] flex flex-col gap-4 p-5 pt-[20px] sm:p-6 sm:pt-[24px]">
        <div className="absolute left-4 right-4 top-[-16px] z-10 -translate-y-[62%] sm:left-6 sm:right-6">
          <div className="flex flex-wrap items-end gap-2">
            <button
              type="button"
              disabled
              className={`rounded-t-lg border border-b-0 px-5 py-2.5 text-sm font-medium transition-colors disabled:opacity-100 ${
                activeGameTab === 'minecraft'
                  ? 'border-[var(--border)] bg-[var(--surface)]'
                  : 'border-[var(--border)]/50 bg-[var(--surface)]/40 opacity-60'
              }`}
            >
              Minecraft
            </button>
            <button
              type="button"
              disabled
              className="rounded-t-lg border border-b-0 border-[var(--border)]/50 bg-[var(--surface)]/40 px-5 py-2.5 text-sm font-medium opacity-60"
            >
              More soon
            </button>
          </div>
        </div>

        <div className="flex items-center justify-between gap-3">
          <div>
            <h2 className="text-xl font-semibold">Known servers</h2>
            <p className="text-sm muted">Visible instances for your account.</p>
          </div>
          <span className="chip">{servers.length}</span>
        </div>

        {loading ? (
          <div className="panel-soft rounded-xl px-4 py-3 text-sm muted">Loading server records…</div>
        ) : servers.length === 0 ? (
          <div className="panel-soft rounded-xl px-4 py-3 text-sm muted">
            No Minecraft servers exist yet.
          </div>
        ) : (
          <div className="space-y-4">
            {servers.map((server) => {
              const expanded = selectedServerId === server.id;
              const indicator = getServerIndicator(server);
              const progressMessage = getServerProgressMessage(server);
              const needsProvisioning =
                server.observed_state === 'draft' ||
                server.observed_state === 'unprovisioned' ||
                server.observed_state === 'provisioning' ||
                server.observed_state === 'importing';
              const lifecycleSupported = runtimeCapabilities?.lifecycle_supported ?? true;
              const statusSupported = runtimeCapabilities?.status_supported ?? true;
              const deleteSupported = runtimeCapabilities?.delete_supported ?? true;
              const canDelete = me.role === 'admin';
              const canStart =
                lifecycleSupported &&
                server.observed_state !== 'provisioning' &&
                server.observed_state !== 'importing' &&
                server.observed_state !== 'running' &&
                server.observed_state !== 'starting' &&
                server.observed_state !== 'restarting';
              const canRestart =
                lifecycleSupported &&
                !needsProvisioning &&
                (server.observed_state === 'running' || server.health_state === 'healthy');
              const canStop =
                lifecycleSupported &&
                !needsProvisioning &&
                (server.observed_state === 'running' ||
                  server.observed_state === 'starting' ||
                  server.observed_state === 'restarting');
              return (
                <div
                  key={server.id}
                  className={`panel-soft rounded-xl px-4 py-4 transition ${
                    expanded ? 'border-[var(--orange-soft)] bg-white/10' : ''
                  }`}
                >
                  <div className="flex flex-col gap-4">
                    <div className="flex flex-wrap items-start justify-between gap-4">
                      <div className="min-w-0 flex-1">
                        <div className="flex flex-wrap items-center gap-3">
                          <h3 className="text-lg font-semibold text-white">{server.display_name}</h3>
                          <span
                            className={`inline-flex items-center gap-2 rounded-full border border-white/10 bg-white/5 px-3 py-1 text-xs font-medium ${indicator.textClass}`}
                          >
                            <span
                              className={`inline-flex h-3 w-3 shrink-0 items-center justify-center rounded-full ${
                                indicator.animated ? `rf-server-status-pulse ${indicator.dotClass}` : indicator.dotClass
                              }`}
                            />
                            {indicator.label}
                          </span>
                          <span className="chip">Port {server.listen_port}</span>
                          <span className="chip">
                            {server.current_player_count}/{server.max_player_count ?? 0} players
                          </span>
                          <span className="chip">{server.server_distribution} {server.minecraft_version}</span>
                        </div>
                      </div>

                      <div className="grid w-full grid-cols-2 gap-2 sm:flex sm:w-auto sm:min-w-fit sm:flex-wrap sm:justify-end sm:self-start">
                        <button
                          type="button"
                          className="btn-secondary w-full px-3 py-2 text-center text-xs disabled:opacity-50 sm:w-auto"
                          disabled={
                            statusRefreshingServerId !== null || actionLoading !== null || !statusSupported
                          }
                          onClick={() => void refreshSelectedServerStatus(server.id, true)}
                          title={!statusSupported ? runtimeCapabilities?.reason ?? 'Status refresh unavailable here' : undefined}
                        >
                          {statusRefreshingServerId === server.id ? 'Refreshing…' : 'Refresh Status'}
                        </button>
                        <button
                          type="button"
                          className="btn-primary w-full px-3 py-2 text-center text-xs disabled:opacity-50 sm:w-auto"
                          disabled={actionLoading !== null || !canStart}
                          onClick={() => void handleRequestAction(server, 'start')}
                        >
                          {actionLoading === 'start' && actionServerId === server.id ? 'Starting…' : 'Start'}
                        </button>
                        <button
                          type="button"
                          className="btn-secondary w-full px-3 py-2 text-center text-xs disabled:opacity-50 sm:w-auto"
                          disabled={actionLoading !== null || !canRestart}
                          onClick={() => void handleRequestAction(server, 'restart')}
                          title={
                            needsProvisioning
                              ? 'Provision or import this Minecraft server before restarting it.'
                              : undefined
                          }
                        >
                          {actionLoading === 'restart' && actionServerId === server.id ? 'Restarting…' : 'Restart'}
                        </button>
                        <button
                          type="button"
                          className="btn-secondary w-full px-3 py-2 text-center text-xs disabled:opacity-50 sm:w-auto"
                          disabled={actionLoading !== null || !canStop}
                          onClick={() => void handleRequestAction(server, 'stop')}
                          title={
                            needsProvisioning
                              ? 'Provision or import this Minecraft server before stopping it.'
                              : undefined
                          }
                        >
                          {actionLoading === 'stop' && actionServerId === server.id ? 'Stopping…' : 'Stop'}
                        </button>
                        {canDelete ? (
                          <button
                            type="button"
                            className="rounded-xl border border-red-400/30 bg-red-500/10 px-3 py-2 text-center text-xs font-medium text-red-100 transition hover:border-red-300/50 hover:bg-red-500/15 disabled:opacity-50 sm:w-auto"
                            disabled={deletingServerId !== null || actionLoading !== null || !deleteSupported}
                            onClick={() => setDeleteConfirmServer(server)}
                            title={!deleteSupported ? runtimeCapabilities?.reason ?? 'Delete unavailable here' : undefined}
                          >
                            Delete
                          </button>
                        ) : null}
                        <button
                          type="button"
                          className="btn-ghost col-span-2 w-full px-3 py-2 text-center text-xs sm:col-auto sm:min-w-[7.75rem] sm:w-auto"
                          onClick={() => {
                            setServerEdits((prev) => ({
                              ...prev,
                              [server.id]: prev[server.id] ?? serverToSettingsForm(server),
                            }));
                            setSelectedServerId((current) => (current === server.id ? null : server.id));
                          }}
                        >
                          {expanded ? 'Hide details' : 'Show details'}
                        </button>
                      </div>
                    </div>

                    {(server.description || progressMessage) ? (
                      <div className="w-full space-y-2">
                        {server.description ? (
                          <div className="w-full text-sm muted">{server.description}</div>
                        ) : null}
                        {progressMessage ? (
                          <div
                            className={`w-full text-sm leading-6 ${
                              server.health_state === 'error' ||
                              server.observed_state === 'failed' ||
                              server.observed_state === 'error'
                                ? 'text-rose-200'
                                : 'text-amber-100'
                            }`}
                          >
                            {progressMessage}
                          </div>
                        ) : null}
                      </div>
                    ) : null}

                    {expanded ? (
                      <div className="grid gap-4 xl:grid-cols-[minmax(18rem,0.9fr)_minmax(24rem,1.35fr)]">
                        <section className="panel-soft rounded-xl px-4 py-4">
                          <div className="mb-3 flex items-center justify-between gap-3">
                            <h4 className="text-sm font-semibold uppercase tracking-[0.18em] text-white/80">
                              Server overview
                            </h4>
                            <span className="chip text-[11px]">
                              {server.server_distribution} {server.minecraft_version}
                            </span>
                          </div>
                          <dl className="grid gap-x-4 gap-y-3 text-sm sm:grid-cols-2 xl:grid-cols-1 2xl:grid-cols-2">
                            <div>
                              <dt className="muted text-[11px] uppercase tracking-[0.16em]">Owner</dt>
                              <dd className="mt-1 text-white">{server.owner_display_name}</dd>
                            </div>
                            <div>
                              <dt className="muted text-[11px] uppercase tracking-[0.16em]">World</dt>
                              <dd className="mt-1 text-white">{server.world_name}</dd>
                            </div>
                            <div>
                              <dt className="muted text-[11px] uppercase tracking-[0.16em]">Address</dt>
                              <dd className="mt-1 text-white">{server.listen_host}:{server.listen_port}</dd>
                            </div>
                            <div>
                              <dt className="muted text-[11px] uppercase tracking-[0.16em]">Memory</dt>
                              <dd className="mt-1 text-white">{server.min_memory_mb}-{server.max_memory_mb} MB</dd>
                            </div>
                            <div>
                              <dt className="muted text-[11px] uppercase tracking-[0.16em]">Players</dt>
                              <dd className="mt-1 text-white">
                                {server.current_player_count} / {server.max_player_count ?? '—'}
                              </dd>
                            </div>
                            <div>
                              <dt className="muted text-[11px] uppercase tracking-[0.16em]">Last started</dt>
                              <dd className="mt-1 text-white">{formatTs(server.last_started_ts)}</dd>
                            </div>
                            <div>
                              <dt className="muted text-[11px] uppercase tracking-[0.16em]">Last stopped</dt>
                              <dd className="mt-1 text-white">{formatTs(server.last_stopped_ts)}</dd>
                            </div>
                            <div>
                              <dt className="muted text-[11px] uppercase tracking-[0.16em]">MOTD</dt>
                              <dd className="mt-1 text-white">{server.motd || 'Defaults to display name'}</dd>
                            </div>
                          </dl>
                        </section>

                        {me.role === 'admin' ? (
                          <section className="rounded-xl border border-[var(--border)] bg-[var(--panel)]/45 px-4 py-4">
                            <div className="flex flex-wrap items-start justify-between gap-3">
                              <div className="space-y-1">
                                <h4 className="text-sm font-semibold uppercase tracking-[0.18em] text-white/80">
                                  Server settings
                                </h4>
                                <p className="max-w-2xl text-xs muted">
                                  Save writes the managed Minecraft configuration to the Debian host. Restart the server afterwards if you need runtime changes applied immediately.
                                </p>
                              </div>
                              <button
                                type="button"
                                className="btn-primary px-4 py-2 text-sm disabled:opacity-50"
                                disabled={savingServerId === server.id}
                                onClick={() => void handleSaveServerSettings(server)}
                              >
                                {savingServerId === server.id ? 'Saving…' : 'Save settings'}
                              </button>
                            </div>

                            <div className="mt-4 grid gap-4 lg:grid-cols-[minmax(0,1fr)_minmax(0,1.1fr)]">
                              <div className="grid gap-3 sm:grid-cols-2">
                                <label className="space-y-1.5">
                                  <span className="text-xs font-medium uppercase tracking-[0.16em] text-white/80">
                                    Gamemode
                                  </span>
                                  <select
                                    className="select rounded-xl px-3 py-2.5"
                                    value={(serverEdits[server.id] ?? serverToSettingsForm(server)).gamemode}
                                    onChange={(event) =>
                                      setServerEdit(
                                        server.id,
                                        'gamemode',
                                        event.target.value as ServerSettingsFormState['gamemode'],
                                      )
                                    }
                                  >
                                    <option value="survival">Survival</option>
                                    <option value="creative">Creative</option>
                                    <option value="adventure">Adventure</option>
                                    <option value="spectator">Spectator</option>
                                  </select>
                                </label>

                                <label className="space-y-1.5">
                                  <span className="text-xs font-medium uppercase tracking-[0.16em] text-white/80">
                                    Difficulty
                                  </span>
                                  <select
                                    className="select rounded-xl px-3 py-2.5"
                                    value={(serverEdits[server.id] ?? serverToSettingsForm(server)).difficulty}
                                    onChange={(event) =>
                                      setServerEdit(
                                        server.id,
                                        'difficulty',
                                        event.target.value as ServerSettingsFormState['difficulty'],
                                      )
                                    }
                                  >
                                    <option value="peaceful">Peaceful</option>
                                    <option value="easy">Easy</option>
                                    <option value="normal">Normal</option>
                                    <option value="hard">Hard</option>
                                  </select>
                                </label>

                                <label className="space-y-1.5">
                                  <span className="text-xs font-medium uppercase tracking-[0.16em] text-white/80">
                                    Max players
                                  </span>
                                  <input
                                    className="input rounded-xl px-3 py-2.5"
                                    type="number"
                                    min={1}
                                    max={500}
                                    value={(serverEdits[server.id] ?? serverToSettingsForm(server)).max_player_count}
                                    onChange={(event) => setServerEdit(server.id, 'max_player_count', event.target.value)}
                                  />
                                </label>

                                <label className="space-y-1.5 sm:col-span-2">
                                  <span className="text-xs font-medium uppercase tracking-[0.16em] text-white/80">
                                    Message of the day
                                  </span>
                                  <input
                                    className="input rounded-xl px-3 py-2.5"
                                    value={(serverEdits[server.id] ?? serverToSettingsForm(server)).motd}
                                    onChange={(event) => setServerEdit(server.id, 'motd', event.target.value)}
                                  />
                                </label>
                              </div>

                              <div className="grid gap-2 sm:grid-cols-2">
                                {SERVER_SETTINGS_TOGGLE_FIELDS.map(({ key, label }) => (
                                  <label
                                    key={key}
                                    className="panel-soft flex min-h-[3rem] items-center gap-3 rounded-xl px-3 py-2 text-sm"
                                  >
                                    <input
                                      type="checkbox"
                                      checked={(serverEdits[server.id] ?? serverToSettingsForm(server))[key]}
                                      onChange={(event) => setServerEdit(server.id, key, event.target.checked)}
                                    />
                                    <span>{label}</span>
                                  </label>
                                ))}
                              </div>
                            </div>
                          </section>
                        ) : (
                          <section className="panel-soft rounded-xl px-4 py-4 text-sm muted">
                            Server settings can be changed by admins. Runtime controls remain available from the server row.
                          </section>
                        )}
                      </div>
                    ) : null}
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </section>

      <section className="panel flex min-h-[34rem] flex-col gap-4 p-5 sm:p-6">
        <div>
          <h2 className="text-xl font-semibold">Create Minecraft server</h2>
          <p className="text-sm muted">
            Choose whether Rustyfin should create a new managed server or import an existing one from the Debian host, then walk through the setup step by step.
          </p>
        </div>

        {me.role !== 'admin' ? (
          <div className="panel-soft rounded-xl px-4 py-3 text-sm muted">
            Only admins can create or import server records. Users can still start, stop, and restart visible servers from the known servers list.
          </div>
        ) : (
          <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto pr-1">
            <div className="panel-soft space-y-3 p-4 sm:p-5">
              <div className="flex items-center justify-between gap-3">
                <span className="text-xs muted">
                  {normalizedCreateStepIndex + 1}/{createSteps.length}
                </span>
              </div>
              <div className="h-2 overflow-hidden rounded-full bg-white/10">
                <div
                  className="h-full rounded-full bg-gradient-to-r from-[var(--orange)] to-[var(--purple)]"
                  style={{ width: `${Math.max(createProgressPercent, 8)}%` }}
                />
              </div>
              <div className="grid grid-cols-2 gap-2 sm:grid-cols-3 xl:grid-cols-6">
                {createSteps.map((stepId, index) => (
                  <div
                    key={stepId}
                    className={`chip justify-center text-center ${
                      index === normalizedCreateStepIndex ? 'chip-accent' : ''
                    }`}
                  >
                    {CREATE_WIZARD_STEP_META[stepId].title}
                  </div>
                ))}
              </div>
            </div>

            <section className={`${activeCreateStepId === 'mode' ? 'panel-soft' : 'panel'} space-y-6 p-6 sm:p-7`}>
              <div>
                <h3 className="text-2xl font-semibold sm:text-3xl">{activeCreateStep.title}</h3>
                <p className="mt-2 text-sm muted">{activeCreateStep.description}</p>
              </div>

              {activeCreateStepErrors.length > 0 && activeCreateStepId !== 'mode' ? (
                <div className="rounded-xl border border-amber-400/30 bg-amber-500/10 px-4 py-3 text-sm text-amber-100">
                  <ul className="list-disc space-y-1 pl-5">
                    {activeCreateStepErrors.map((entry) => (
                      <li key={entry}>{entry}</li>
                    ))}
                  </ul>
                </div>
              ) : null}

              {activeCreateStepId === 'mode' ? (
                <div className="grid gap-4 sm:grid-cols-2">
                  <button
                    type="button"
                    className={`rounded-2xl border px-5 py-5 text-left transition ${
                      createMode === 'create'
                        ? 'border-transparent bg-[linear-gradient(rgba(26,31,53,0.82),rgba(26,31,53,0.82))_padding-box,linear-gradient(110deg,rgba(255,145,77,0.95)_0%,rgba(255,117,136,0.95)_100%)_border-box]'
                        : 'border-[var(--border)]/70 bg-[var(--surface)]/70 hover:border-[var(--orange-soft)]/55'
                    }`}
                    onClick={() => {
                      setCreateMode('create');
                      setPendingImportServerId(null);
                    }}
                  >
                    <div className="text-base font-semibold text-white">Create new managed server</div>
                    <p className="mt-2 text-sm muted">
                      Rustyfin generates the server files, installs the native service, and launches it when you click Start.
                    </p>
                  </button>
                  <button
                    type="button"
                    className={`rounded-2xl border px-5 py-5 text-left transition ${
                      createMode === 'import'
                        ? 'border-transparent bg-[linear-gradient(rgba(26,31,53,0.82),rgba(26,31,53,0.82))_padding-box,linear-gradient(110deg,rgba(255,145,77,0.95)_0%,rgba(255,117,136,0.95)_100%)_border-box]'
                        : 'border-[var(--border)]/70 bg-[var(--surface)]/70 hover:border-[var(--orange-soft)]/55'
                    }`}
                    onClick={() => setCreateMode('import')}
                  >
                    <div className="text-base font-semibold text-white">Import existing server</div>
                    <p className="mt-2 text-sm muted">
                      Rustyfin creates the managed record, then imports a prepared Minecraft server directory from the Debian host.
                    </p>
                  </button>
                </div>
              ) : null}

              {activeCreateStepId === 'core' ? (
                <div className="grid gap-4 sm:grid-cols-2">
                  <label className="space-y-2">
                    <span className="text-sm font-medium text-white">Distribution</span>
                    <select
                      className="select rounded-xl px-4 py-3"
                      value={form.server_distribution}
                      onChange={(event) =>
                        updateForm(
                          'server_distribution',
                          event.target.value as CreateFormState['server_distribution'],
                        )
                      }
                    >
                      <option value="paper">Paper</option>
                      <option value="vanilla">Vanilla</option>
                    </select>
                  </label>
                  <label className="space-y-2">
                    <span className="text-sm font-medium text-white">Minecraft version</span>
                    <input
                      className="input rounded-xl px-4 py-3"
                      value={form.minecraft_version}
                      onChange={(event) => updateForm('minecraft_version', event.target.value)}
                    />
                  </label>
                  <label className="space-y-2">
                    <span className="text-sm font-medium text-white">Port</span>
                    <input
                      className="input rounded-xl px-4 py-3"
                      type="number"
                      value={form.listen_port}
                      onChange={(event) => updateForm('listen_port', event.target.value)}
                    />
                  </label>
                  <label className="space-y-2">
                    <span className="text-sm font-medium text-white">Display name</span>
                    <input
                      className="input rounded-xl px-4 py-3"
                      value={form.display_name}
                      onChange={(event) => updateForm('display_name', event.target.value)}
                      placeholder="Example: Family SMP"
                    />
                  </label>
                  <label className="space-y-2 sm:col-span-2">
                    <span className="text-sm font-medium text-white">World name</span>
                    <input
                      className="input rounded-xl px-4 py-3"
                      value={form.world_name}
                      onChange={(event) => updateForm('world_name', event.target.value)}
                      placeholder="Example: family-world"
                    />
                  </label>
                </div>
              ) : null}

              {activeCreateStepId === 'gameplay' ? (
                <div className="grid gap-4 sm:grid-cols-2">
                  <label className="space-y-2">
                    <span className="text-sm font-medium text-white">Gamemode</span>
                    <select
                      className="select rounded-xl px-4 py-3"
                      value={form.gamemode}
                      onChange={(event) =>
                        updateForm('gamemode', event.target.value as CreateFormState['gamemode'])
                      }
                    >
                      <option value="survival">Survival</option>
                      <option value="creative">Creative</option>
                      <option value="adventure">Adventure</option>
                      <option value="spectator">Spectator</option>
                    </select>
                  </label>
                  <label className="space-y-2">
                    <span className="text-sm font-medium text-white">Difficulty</span>
                    <select
                      className="select rounded-xl px-4 py-3"
                      value={form.difficulty}
                      onChange={(event) =>
                        updateForm('difficulty', event.target.value as CreateFormState['difficulty'])
                      }
                    >
                      <option value="peaceful">Peaceful</option>
                      <option value="easy">Easy</option>
                      <option value="normal">Normal</option>
                      <option value="hard">Hard</option>
                    </select>
                  </label>
                  <label className="space-y-2 sm:col-span-2">
                    <span className="text-sm font-medium text-white">Description</span>
                    <textarea
                      className="input min-h-[5.5rem] rounded-xl px-4 py-3"
                      value={form.description}
                      onChange={(event) => updateForm('description', event.target.value)}
                      placeholder="Optional notes about this server."
                    />
                  </label>
                  <label className="space-y-2 sm:col-span-2">
                    <span className="text-sm font-medium text-white">Message of the day</span>
                    <input
                      className="input rounded-xl px-4 py-3"
                      value={form.motd}
                      onChange={(event) => updateForm('motd', event.target.value)}
                      placeholder="Defaults to the display name if left blank."
                    />
                  </label>
                  <label className="panel-soft flex items-center gap-3 rounded-xl px-4 py-3 text-sm">
                    <input
                      type="checkbox"
                      checked={form.hardcore}
                      onChange={(event) => updateForm('hardcore', event.target.checked)}
                    />
                    <span>Hardcore mode</span>
                  </label>
                  <label className="panel-soft flex items-center gap-3 rounded-xl px-4 py-3 text-sm">
                    <input
                      type="checkbox"
                      checked={form.pvp}
                      onChange={(event) => updateForm('pvp', event.target.checked)}
                    />
                    <span>PVP enabled</span>
                  </label>
                </div>
              ) : null}

              {activeCreateStepId === 'resources' ? (
                <div className="grid gap-4 sm:grid-cols-2">
                  <label className="space-y-2">
                    <span className="text-sm font-medium text-white">Min memory (MB)</span>
                    <input
                      className="input rounded-xl px-4 py-3"
                      type="number"
                      value={form.min_memory_mb}
                      onChange={(event) => updateForm('min_memory_mb', event.target.value)}
                    />
                  </label>
                  <label className="space-y-2">
                    <span className="text-sm font-medium text-white">Max memory (MB)</span>
                    <input
                      className="input rounded-xl px-4 py-3"
                      type="number"
                      value={form.max_memory_mb}
                      onChange={(event) => updateForm('max_memory_mb', event.target.value)}
                    />
                  </label>
                  <label className="space-y-2 sm:col-span-2">
                    <span className="text-sm font-medium text-white">Max players</span>
                    <input
                      className="input rounded-xl px-4 py-3"
                      type="number"
                      value={form.max_player_count}
                      onChange={(event) => updateForm('max_player_count', event.target.value)}
                    />
                  </label>
                  <div className="panel-soft rounded-xl px-4 py-3 text-sm muted sm:col-span-2">
                    Rustyfin will use these limits to generate the managed server runtime on the Debian host.
                  </div>
                </div>
              ) : null}

              {activeCreateStepId === 'source' ? (
                <div className="flex flex-col gap-4">
                  {pendingImportServerId ? (
                    <div className="panel-soft rounded-xl px-4 py-3 text-sm">
                      <div className="font-medium text-white">Retrying import into the existing draft</div>
                      <div className="mt-1 text-xs muted">
                        Rustyfin already created the managed record. Fix the path below and retry the import without creating another draft.
                      </div>
                    </div>
                  ) : (
                    <div className="panel-soft rounded-xl px-4 py-3 text-sm muted">
                      Rustyfin will create the managed server record first, then import this host path into it.
                    </div>
                  )}

                  <label className="space-y-2">
                    <span className="text-sm font-medium text-white">Host source path</span>
                    <input
                      className="input rounded-xl px-4 py-3"
                      value={importSourcePath}
                      onChange={(event) => setImportSourcePath(event.target.value)}
                      placeholder="/srv/minecraft/existing-world"
                    />
                  </label>

                  <div className="flex flex-wrap gap-2">
                    <button
                      type="button"
                      className="btn-secondary px-4 py-2 text-sm"
                      onClick={() => openHostDirectoryBrowser(importSourcePath)}
                    >
                      Browse Host Directories
                    </button>
                  </div>
                </div>
              ) : null}

              {activeCreateStepId === 'review' ? (
                <div className="flex flex-col gap-4">
                  <div className="grid gap-4 sm:grid-cols-2">
                    <div className="panel rounded-2xl p-4">
                      <div className="text-xs uppercase tracking-[0.24em] muted">Server</div>
                      <dl className="mt-3 space-y-2 text-sm">
                        <div className="flex items-center justify-between gap-4">
                          <dt className="muted">Mode</dt>
                          <dd className="text-right text-white">
                            {createMode === 'import' ? 'Import existing server' : 'Create managed server'}
                          </dd>
                        </div>
                        <div className="flex items-center justify-between gap-4">
                          <dt className="muted">Name</dt>
                          <dd className="text-right text-white">{form.display_name || 'Not set'}</dd>
                        </div>
                        <div className="flex items-center justify-between gap-4">
                          <dt className="muted">Distribution</dt>
                          <dd className="text-right text-white">{titleCase(form.server_distribution)}</dd>
                        </div>
                        <div className="flex items-center justify-between gap-4">
                          <dt className="muted">Version</dt>
                          <dd className="text-right text-white">{form.minecraft_version || 'Not set'}</dd>
                        </div>
                        <div className="flex items-center justify-between gap-4">
                          <dt className="muted">World</dt>
                          <dd className="text-right text-white">{form.world_name || 'Not set'}</dd>
                        </div>
                        <div className="flex items-center justify-between gap-4">
                          <dt className="muted">Port</dt>
                          <dd className="text-right text-white">{form.listen_port || 'Not set'}</dd>
                        </div>
                        {createMode === 'import' ? (
                          <div className="flex items-center justify-between gap-4">
                            <dt className="muted">Source path</dt>
                            <dd className="text-right text-white">{importSourcePath || 'Not set'}</dd>
                          </div>
                        ) : null}
                      </dl>
                    </div>

                    <div className="panel rounded-2xl p-4">
                      <div className="text-xs uppercase tracking-[0.24em] muted">Gameplay and resources</div>
                      <dl className="mt-3 space-y-2 text-sm">
                        <div className="flex items-center justify-between gap-4">
                          <dt className="muted">Gamemode</dt>
                          <dd className="text-right text-white">{titleCase(form.gamemode)}</dd>
                        </div>
                        <div className="flex items-center justify-between gap-4">
                          <dt className="muted">Difficulty</dt>
                          <dd className="text-right text-white">{titleCase(form.difficulty)}</dd>
                        </div>
                        <div className="flex items-center justify-between gap-4">
                          <dt className="muted">RAM</dt>
                          <dd className="text-right text-white">
                            {form.min_memory_mb} MB to {form.max_memory_mb} MB
                          </dd>
                        </div>
                        <div className="flex items-center justify-between gap-4">
                          <dt className="muted">Max players</dt>
                          <dd className="text-right text-white">{form.max_player_count}</dd>
                        </div>
                        <div className="flex items-center justify-between gap-4">
                          <dt className="muted">MOTD</dt>
                          <dd className="text-right text-white">{form.motd || 'Defaults to display name'}</dd>
                        </div>
                      </dl>
                    </div>
                  </div>

                  <div className="grid gap-3 sm:grid-cols-2">
                    {TOGGLE_FIELDS.map(({ key, label }) => (
                      <label key={key} className="panel-soft flex items-center gap-3 rounded-xl px-4 py-3 text-sm">
                        <input
                          type="checkbox"
                          checked={form[key]}
                          onChange={(event) => updateForm(key, event.target.checked)}
                        />
                        <span>{label}</span>
                      </label>
                    ))}
                    <label className="panel-soft flex items-center gap-3 rounded-xl border border-[var(--orange-soft)]/40 px-4 py-3 text-sm sm:col-span-2">
                      <input
                        type="checkbox"
                        checked={form.eula_accepted}
                        onChange={(event) => updateForm('eula_accepted', event.target.checked)}
                      />
                      <span>I confirm the Minecraft EULA has been accepted.</span>
                    </label>
                  </div>

                  <div className="panel-soft rounded-xl px-4 py-3 text-sm muted">
                    {createMode === 'import'
                      ? 'Rustyfin will create the managed server record, then import the host directory into it and install the native unit.'
                      : 'Creating the server adds a draft record to known servers immediately. Clicking Start provisions and launches the managed server automatically.'}
                  </div>
                </div>
              ) : null}

              <div className="flex flex-wrap items-center justify-between gap-3 border-t border-white/8 pt-2">
                <button
                  type="button"
                  className="btn-secondary px-4 py-2 text-sm disabled:opacity-50"
                  disabled={normalizedCreateStepIndex === 0}
                  onClick={goToPreviousCreateStep}
                >
                  Back
                </button>
                <div className="flex flex-wrap items-center gap-3">
                  {activeCreateStepId !== 'review' ? (
                    <button
                      type="button"
                      className="btn-primary px-5 py-2.5 text-sm disabled:opacity-60"
                      disabled={!canAdvanceCreateStep}
                      onClick={goToNextCreateStep}
                    >
                      Continue
                    </button>
                  ) : (
                    <button
                      type="button"
                      className="btn-primary px-5 py-2.5 text-sm disabled:opacity-60"
                      disabled={
                        creating ||
                        importing ||
                        activeCreateStepErrors.length > 0 ||
                        (createMode === 'import' && !(runtimeCapabilities?.import_supported ?? true))
                      }
                      onClick={() => void handleCreateServer()}
                    >
                      {creating || importing
                        ? createMode === 'import'
                          ? 'Importing…'
                          : 'Creating…'
                        : createMode === 'import'
                          ? 'Create and Import Server'
                          : 'Create Draft Server'}
                    </button>
                  )}
                </div>
              </div>
            </section>
          </div>
        )}
      </section>

      <ConfirmModal
        open={Boolean(deleteConfirmServer)}
        title="Delete Server"
        description="Delete this Minecraft server record and remove its managed host files? This action is destructive and cannot be undone."
        confirmLabel={
          deleteConfirmServer && deletingServerId === deleteConfirmServer.id
            ? 'Deleting…'
            : 'Delete server'
        }
        confirmDisabled={Boolean(deleteConfirmServer && deletingServerId === deleteConfirmServer.id)}
        cancelDisabled={Boolean(deleteConfirmServer && deletingServerId === deleteConfirmServer.id)}
        destructive
        maxWidthClassName="max-w-md"
        zIndexClassName="z-[150]"
        onCancel={() => setDeleteConfirmServer(null)}
        onConfirm={() => {
          if (!deleteConfirmServer) return;
          void handleDeleteServer(deleteConfirmServer);
        }}
      >
        {deleteConfirmServer ? (
          <div className="panel-soft rounded-xl px-4 py-3 text-sm">
            <div className="font-medium text-white">{deleteConfirmServer.display_name}</div>
            <div className="mt-1 text-xs muted">
              {deleteConfirmServer.server_distribution} {deleteConfirmServer.minecraft_version}
              {' · '}
              {deleteConfirmServer.world_name}
              {' · '}
              Port {deleteConfirmServer.listen_port}
            </div>
          </div>
        ) : null}
      </ConfirmModal>

      {hostBrowser.open ? (
        <div className="fixed inset-0 z-[150] flex items-center justify-center overflow-y-auto bg-black/70 p-4 sm:p-6">
          <div className="panel my-auto flex max-h-[calc(100vh-2rem)] w-full max-w-3xl flex-col gap-4 overflow-hidden rounded-2xl p-5 sm:max-h-[calc(100vh-3rem)] sm:p-6">
            <div className="flex shrink-0 items-center justify-between gap-3">
              <div>
                <h2 className="text-xl font-semibold">Browse Backend Directories</h2>
                <p className="text-sm muted">
                  Choose the host directory that contains the existing Minecraft server you want to import.
                </p>
              </div>
              <button
                type="button"
                onClick={closeHostDirectoryBrowser}
                className="btn-ghost px-3 py-2 text-sm"
              >
                Close
              </button>
            </div>

            {hostBrowser.roots.length > 0 ? (
              <div className="max-h-24 shrink-0 overflow-y-auto">
                <div className="flex flex-wrap gap-2">
                  {hostBrowser.roots.map((rootPath) => (
                    <button
                      key={rootPath}
                      type="button"
                      onClick={() => navigateHostDirectory(rootPath)}
                      className={`btn-ghost px-2.5 py-1 text-xs ${
                        hostBrowser.currentPath.startsWith(rootPath)
                          ? 'border-[var(--orange-soft)] text-[var(--orange-soft)]'
                          : ''
                      }`}
                    >
                      {rootPath}
                    </button>
                  ))}
                </div>
              </div>
            ) : null}

            <div className="panel-soft flex shrink-0 items-center gap-2 rounded-xl border border-[var(--border)] px-3 py-2">
              <button
                type="button"
                onClick={() => navigateHostDirectory(hostBrowser.parentPath)}
                disabled={!hostBrowser.parentPath || hostBrowser.loading}
                className="btn-secondary px-3 py-1 text-xs disabled:opacity-50"
              >
                Up
              </button>
              <div className="min-w-0">
                <p className="text-[11px] uppercase tracking-[0.12em] muted">Current Path</p>
                <p className="text-sm font-mono truncate" title={hostBrowser.currentPath}>
                  {hostBrowser.currentPath || '—'}
                </p>
              </div>
            </div>

            {hostBrowser.error ? (
              <p className="shrink-0 text-sm text-red-300">{hostBrowser.error}</p>
            ) : null}

            <div className="panel-soft min-h-[260px] flex-1 overflow-auto rounded-xl border border-[var(--border)] p-2">
              {hostBrowser.loading ? (
                <p className="px-2 py-2 text-sm muted">Loading directories…</p>
              ) : hostBrowser.directories.length === 0 ? (
                <p className="px-2 py-2 text-sm muted">No child directories found.</p>
              ) : (
                <div className="space-y-1">
                  {hostBrowser.directories.map((entry) => (
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

            <div className="flex shrink-0 items-center justify-end gap-2">
              <button
                type="button"
                onClick={closeHostDirectoryBrowser}
                className="btn-ghost px-4 py-2 text-sm"
              >
                Cancel
              </button>
              <button
                type="button"
                onClick={() => {
                  setImportSourcePath(hostBrowser.currentPath);
                  closeHostDirectoryBrowser();
                }}
                disabled={hostBrowser.loading || !hostBrowser.currentPath}
                className="btn-primary px-4 py-2 text-sm disabled:opacity-50"
              >
                Use This Directory
              </button>
            </div>
          </div>
        </div>
      ) : null}
    </main>
  );
}

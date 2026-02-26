import { useCallback, useEffect, useMemo, useState } from 'react';

import { apiJson } from '@/lib/api';
import type { Me } from '@/lib/auth';
import { clientErrorMessage } from '@/lib/errors';
import {
  ReconfigureWatchPartyRoomRequest,
  WatchPartyRoomResponse,
  WsRoomReconfiguredMessage,
  getEligibleLibraries,
  reconfigureWatchPartyRoom,
} from '@/lib/watchPartyApi';
import type { MediaItemNode, MediaLibrary } from '../components/MediaPicker';
import type { RoomMode } from '../realtimeTypes';

type UseRoomReconfigureArgs = {
  roomId: string;
  room: WatchPartyRoomResponse | null;
  me: Me | null;
  joinedRole: string | null;
  activeWatchSource: 'video' | 'youtube' | 'web';
  setError: (message: string) => void;
  setInfo: (message: string) => void;
  setInfoForDuration: (message: string, durationMs: number) => void;
};

export function useRoomReconfigure({
  roomId,
  room,
  me,
  joinedRole,
  activeWatchSource,
  setError,
  setInfo,
  setInfoForDuration,
}: UseRoomReconfigureArgs) {
  const [allLibraries, setAllLibraries] = useState<MediaLibrary[]>([]);
  const [eligibleLibraryIds, setEligibleLibraryIds] = useState<string[]>([]);
  const [reconfigureMode, setReconfigureMode] = useState<RoomMode>('video');
  const [reconfigureVideoLibraryId, setReconfigureVideoLibraryId] = useState('');
  const [reconfigureVideoItem, setReconfigureVideoItem] = useState<MediaItemNode | null>(null);
  const [reconfigureAudioLibraryId, setReconfigureAudioLibraryId] = useState('');
  const [reconfigureCreateTool, setReconfigureCreateTool] = useState<'text' | 'canvas'>('text');
  const [reconfigureDirty, setReconfigureDirty] = useState(false);
  const [reconfiguring, setReconfiguring] = useState(false);
  const [reconfigureModalOpen, setReconfigureModalOpen] = useState(false);

  const reconfigureVideoLibraries = useMemo(
    () =>
      allLibraries.filter(
        (library) => library.kind !== 'music' && eligibleLibraryIds.includes(library.id),
      ),
    [allLibraries, eligibleLibraryIds],
  );

  const reconfigureMusicLibraries = useMemo(
    () =>
      allLibraries.filter(
        (library) => library.kind === 'music' && eligibleLibraryIds.includes(library.id),
      ),
    [allLibraries, eligibleLibraryIds],
  );

  const isWatchReconfigureMode = reconfigureMode === 'video';

  const markDirty = useCallback(() => {
    setReconfigureDirty(true);
  }, []);

  const selectReconfigureMode = useCallback((mode: RoomMode) => {
    setReconfigureDirty(true);
    setReconfigureMode(mode);
  }, []);

  const handleRemoteRoomReconfigured = useCallback((payload: WsRoomReconfiguredMessage) => {
    setReconfigureDirty(false);
    setReconfigureVideoItem(null);
    setReconfigureAudioLibraryId(payload.audio_library_id || '');
    setReconfigureCreateTool(payload.create_tool === 'canvas' ? 'canvas' : 'text');
    const mode =
      payload.room_mode === 'audio' ||
      payload.room_mode === 'youtube' ||
      payload.room_mode === 'web' ||
      payload.room_mode === 'create' ||
      payload.room_mode === 'play'
        ? payload.room_mode
        : 'video';
    setReconfigureMode(mode);
  }, []);

  useEffect(() => {
    if (!room || reconfigureDirty) return;
    const mode =
      room.room_mode === 'audio' ||
      room.room_mode === 'youtube' ||
      room.room_mode === 'web' ||
      room.room_mode === 'create' ||
      room.room_mode === 'play'
        ? room.room_mode
        : 'video';
    setReconfigureMode(mode);
    if (mode === 'audio') {
      setReconfigureAudioLibraryId(room.audio_library_id ?? '');
      return;
    }
    if (mode === 'create') {
      setReconfigureCreateTool(room.create_tool === 'canvas' ? 'canvas' : 'text');
    }
  }, [room, reconfigureDirty]);

  useEffect(() => {
    if (!room || !me || joinedRole !== 'host') return;

    let cancelled = false;

    (async () => {
      try {
        const libraries = await apiJson<MediaLibrary[]>('/libraries');
        if (cancelled) return;
        setAllLibraries(libraries);

        const participantIds = room.members
          .filter(
            (member) =>
              member.user_id !== me.id &&
              member.status !== 'left' &&
              member.status !== 'declined',
          )
          .map((member) => member.user_id);
        const eligible = await getEligibleLibraries(participantIds);
        if (cancelled) return;
        setEligibleLibraryIds(eligible);

        if (!eligible.includes(reconfigureVideoLibraryId)) {
          const defaultVideoLibrary = libraries.find(
            (library) => library.kind !== 'music' && eligible.includes(library.id),
          );
          setReconfigureVideoLibraryId(defaultVideoLibrary?.id ?? '');
          setReconfigureVideoItem(null);
        }

        if (!eligible.includes(reconfigureAudioLibraryId)) {
          const defaultAudioLibrary = libraries.find(
            (library) => library.kind === 'music' && eligible.includes(library.id),
          );
          setReconfigureAudioLibraryId(defaultAudioLibrary?.id ?? '');
        }
      } catch {
        // Non-fatal; panel can still render with current state.
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [room, me, joinedRole, reconfigureVideoLibraryId, reconfigureAudioLibraryId]);

  const handleReconfigureRoom = useCallback(async () => {
    if (joinedRole !== 'host') {
      setError('Only the room host can reconfigure mode.');
      return;
    }

    let payload: ReconfigureWatchPartyRoomRequest;
    if (reconfigureMode === 'audio') {
      payload = {
        room_mode: 'audio',
      };
    } else if (reconfigureMode === 'play') {
      payload = {
        room_mode: 'play',
      };
    } else if (reconfigureMode === 'create') {
      payload = {
        room_mode: 'create',
        create_tool: reconfigureCreateTool,
      };
    } else {
      payload = {
        room_mode: 'video',
      };
    }

    setReconfiguring(true);
    setReconfigureDirty(true);
    setError('');
    setInfo('');
    try {
      await reconfigureWatchPartyRoom(roomId, payload);
      setReconfigureModalOpen(false);
    } catch (err: unknown) {
      setError(clientErrorMessage(err, 'Failed to reconfigure room'));
    } finally {
      setReconfiguring(false);
    }
  }, [joinedRole, reconfigureMode, reconfigureCreateTool, roomId, setError, setInfo]);

  const handleSwitchWatchSource = useCallback(
    async (target: 'video' | 'youtube' | 'web') => {
      if (joinedRole !== 'host') {
        setError('Only the room host can change watch source.');
        return;
      }
      if (!room || target === activeWatchSource) {
        return;
      }

      let payload: ReconfigureWatchPartyRoomRequest;
      if (target === 'video') {
        setReconfigureVideoItem(null);
        payload = { room_mode: 'video' };
      } else if (target === 'youtube') {
        payload = { room_mode: 'youtube' };
      } else {
        payload = { room_mode: 'web', web_url: (room.web_url || '').trim() || undefined };
      }

      setReconfiguring(true);
      setReconfigureDirty(true);
      setError('');
      setInfo('');
      try {
        await reconfigureWatchPartyRoom(roomId, payload);
      } catch (err: unknown) {
        setError(clientErrorMessage(err, 'Failed to switch watch source'));
      } finally {
        setReconfiguring(false);
      }
    },
    [activeWatchSource, joinedRole, room, roomId, setError, setInfo],
  );

  const handleApplyLocalMedia = useCallback(async () => {
    if (joinedRole !== 'host') {
      setError('Only the room host can select local media.');
      return;
    }
    if (!reconfigureVideoItem) {
      setError('Select a movie or episode first.');
      return;
    }

    setReconfiguring(true);
    setReconfigureDirty(true);
    setError('');
    setInfo('');
    try {
      await reconfigureWatchPartyRoom(roomId, {
        room_mode: 'video',
        item_id: reconfigureVideoItem.id,
      });
    } catch (err: unknown) {
      setError(clientErrorMessage(err, 'Failed to apply local media'));
    } finally {
      setReconfiguring(false);
    }
  }, [joinedRole, reconfigureVideoItem, roomId, setError, setInfo]);

  const handleConfigureAudioLibrary = useCallback(
    async (libraryId: string) => {
      if (joinedRole !== 'host') {
        setError('Only the room host can configure offline library search.');
        return;
      }

      setReconfigureAudioLibraryId(libraryId);
      setReconfiguring(true);
      setReconfigureDirty(true);
      setError('');
      try {
        await reconfigureWatchPartyRoom(roomId, {
          room_mode: 'audio',
          audio_library_id: libraryId || undefined,
        });
        setInfoForDuration(
          libraryId
            ? 'Offline library updated for this room.'
            : 'Offline library cleared. Room is now online-only.',
          5000,
        );
      } catch (err: unknown) {
        setError(clientErrorMessage(err, 'Failed to configure offline library'));
      } finally {
        setReconfiguring(false);
      }
    },
    [joinedRole, roomId, setError, setInfoForDuration],
  );

  return {
    allLibraries,
    eligibleLibraryIds,
    reconfigureMode,
    reconfigureVideoLibraryId,
    setReconfigureVideoLibraryId,
    reconfigureVideoItem,
    setReconfigureVideoItem,
    reconfigureAudioLibraryId,
    setReconfigureAudioLibraryId,
    reconfigureCreateTool,
    setReconfigureCreateTool,
    reconfigureDirty,
    markDirty,
    reconfiguring,
    reconfigureModalOpen,
    setReconfigureModalOpen,
    reconfigureVideoLibraries,
    reconfigureMusicLibraries,
    isWatchReconfigureMode,
    selectReconfigureMode,
    handleRemoteRoomReconfigured,
    handleReconfigureRoom,
    handleSwitchWatchSource,
    handleApplyLocalMedia,
    handleConfigureAudioLibrary,
  };
}

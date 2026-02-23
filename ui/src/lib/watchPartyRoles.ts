export type WatchPartyRoomMode = 'video' | 'audio' | 'youtube' | string;

export function isAdminRole(role: string): boolean {
  return role === 'host' || role === 'controller' || role === 'admin';
}

export function nonAdminRoleLabel(roomMode: WatchPartyRoomMode): string {
  if (roomMode === 'audio') return 'Listener';
  if (roomMode === 'youtube') return 'Participant';
  return 'Member';
}

export function roleLabel(role: string, roomMode: WatchPartyRoomMode): string {
  if (isAdminRole(role)) return 'Admin';
  return nonAdminRoleLabel(roomMode);
}


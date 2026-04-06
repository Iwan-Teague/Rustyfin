export type NavigationGroupItem = {
  href: string;
  label: string;
  description: string;
};

export type NavigationGroup = {
  href: string;
  label: string;
  title: string;
  description: string;
  items: NavigationGroupItem[];
};

export const PERSONAL_GROUP: NavigationGroup = {
  href: '/personal',
  label: 'Personal',
  title: 'Personal',
  description: 'Your media, planning, secure storage, and host-level backup controls.',
  items: [
    {
      href: '/libraries',
      label: 'Libraries',
      description: 'Browse your configured libraries and continue watching.',
    },
    {
      href: '/calendar',
      label: 'Calendar',
      description: 'Review upcoming events and manage personal or shared planning.',
    },
    {
      href: '/vault',
      label: 'Vault',
      description: 'Open RustyVault for saved credentials and secure session management.',
    },
    {
      href: '/dictionary',
      label: 'Dictionary',
      description: 'Open a lightweight glossary workspace for words, notes, and definitions.',
    },
    {
      href: '/backups',
      label: 'Backups',
      description: 'Run snapshots, inspect history, and review backup policies.',
    },
  ],
};

export const SOCIAL_GROUP: NavigationGroup = {
  href: '/social',
  label: 'Social',
  title: 'Social',
  description: 'Live chat, voice, and shared-room surfaces for everyone currently online.',
  items: [
    {
      href: '/channels',
      label: 'Channels',
      description: 'Open text channels, voice channels, and active transcripts.',
    },
    {
      href: '/rooms',
      label: 'Rooms',
      description: 'Join live rooms or create a new shared watch, listen, challenge, or create space.',
    },
  ],
};

export const SERVER_GROUP: NavigationGroup = {
  href: '/server',
  label: 'Server',
  title: 'Server',
  description: 'Operational host surfaces for game servers, networking, and package delivery.',
  items: [
    {
      href: '/ai',
      label: 'AI',
      description: 'Open the Rustyfin assistant and grounded runtime tools.',
    },
    {
      href: '/servers',
      label: 'Servers',
      description: 'Manage Minecraft servers, status, lifecycle, and provisioning.',
    },
    {
      href: '/network',
      label: 'Network',
      description: 'Inspect host network settings, addresses, and remote access configuration.',
    },
    {
      href: '/downloads',
      label: 'Downloads',
      description: 'Access Rustyfin packages, extension downloads, and host-published artifacts.',
    },
  ],
};

export const PRIMARY_NAV_GROUPS: NavigationGroup[] = [
  PERSONAL_GROUP,
  SOCIAL_GROUP,
  SERVER_GROUP,
];

export function navigationGroupForPath(pathname: string): NavigationGroup | null {
  for (const group of PRIMARY_NAV_GROUPS) {
    if (pathname === group.href || pathname.startsWith(`${group.href}/`)) {
      return group;
    }
    if (
      group.items.some(
        (item) => pathname === item.href || pathname.startsWith(`${item.href}/`),
      )
    ) {
      return group;
    }
  }
  return null;
}

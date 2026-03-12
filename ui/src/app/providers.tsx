'use client';

import ActivityPresenceProvider from '@/app/components/ActivityPresenceProvider';
import { AuthProvider } from '@/lib/auth';
import { ChannelsProvider } from '@/lib/channelsContext';
import { MusicPlayerProvider } from '@/lib/musicPlayerContext';

export default function Providers({ children }: { children: React.ReactNode }) {
  return (
    <AuthProvider>
      <ActivityPresenceProvider>
        <ChannelsProvider>
          <MusicPlayerProvider>{children}</MusicPlayerProvider>
        </ChannelsProvider>
      </ActivityPresenceProvider>
    </AuthProvider>
  );
}

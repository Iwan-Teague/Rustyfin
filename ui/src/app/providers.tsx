'use client';

import { AuthProvider } from '@/lib/auth';
import { ChannelsProvider } from '@/lib/channelsContext';
import { MusicPlayerProvider } from '@/lib/musicPlayerContext';

export default function Providers({ children }: { children: React.ReactNode }) {
  return (
    <AuthProvider>
      <ChannelsProvider>
        <MusicPlayerProvider>{children}</MusicPlayerProvider>
      </ChannelsProvider>
    </AuthProvider>
  );
}

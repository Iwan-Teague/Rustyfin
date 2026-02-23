'use client';

import { AuthProvider } from '@/lib/auth';
import { ChannelsProvider } from '@/lib/channelsContext';

export default function Providers({ children }: { children: React.ReactNode }) {
  return (
    <AuthProvider>
      <ChannelsProvider>{children}</ChannelsProvider>
    </AuthProvider>
  );
}

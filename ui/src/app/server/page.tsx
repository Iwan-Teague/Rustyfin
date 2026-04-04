'use client';

import NavGroupHubPage from '@/app/components/NavGroupHubPage';
import { SERVER_GROUP } from '@/app/navigationGroups';

export default function ServerPage() {
  return <NavGroupHubPage group={SERVER_GROUP} />;
}

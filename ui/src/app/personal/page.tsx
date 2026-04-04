'use client';

import NavGroupHubPage from '@/app/components/NavGroupHubPage';
import { PERSONAL_GROUP } from '@/app/navigationGroups';

export default function PersonalPage() {
  return <NavGroupHubPage group={PERSONAL_GROUP} />;
}

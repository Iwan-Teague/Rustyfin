'use client';

import type { DictionaryWorkspaceMember } from '@/lib/dictionaryApi';

export type WorkspaceMemberDraft = {
  loginUsername: string;
  role: 'owner' | 'editor' | 'viewer';
};

export function canSubmitWorkspaceMemberDraft(draft: WorkspaceMemberDraft): boolean {
  return draft.loginUsername.trim().length > 0;
}

export function sortedWorkspaceMembers(
  members: DictionaryWorkspaceMember[],
): DictionaryWorkspaceMember[] {
  const roleRank = new Map<string, number>([
    ['owner', 0],
    ['editor', 1],
    ['viewer', 2],
  ]);

  return [...members].sort((left, right) => {
    const leftRank = roleRank.get(left.role) ?? 99;
    const rightRank = roleRank.get(right.role) ?? 99;
    if (leftRank !== rightRank) return leftRank - rightRank;

    const displayCmp = left.display_name.localeCompare(right.display_name);
    if (displayCmp !== 0) return displayCmp;

    return left.login_username.localeCompare(right.login_username);
  });
}

'use client';

import { apiJson } from '@/lib/api';

export type DictionaryWorkspace = {
  id: string;
  space_id: string;
  slug: string;
  title: string;
  workspace_kind: string;
  owner_user_id: string | null;
  is_system_seeded: boolean;
  created_ts: number;
  updated_ts: number;
};

export type DictionaryTreeNode = {
  id: string;
  workspace_id: string;
  parent_node_id: string | null;
  node_kind: string;
  title: string;
  person_id: string | null;
  sort_order: number;
  icon_name: string | null;
  note: string | null;
  is_system_seeded: boolean;
  created_by_user_id: string | null;
  created_ts: number;
  updated_ts: number;
};

export type DictionaryPerson = {
  id: string;
  space_id: string;
  canonical_name: string;
  display_name: string;
  summary: string | null;
  primary_photo_path: string | null;
  primary_photo_content_type: string | null;
  search_text: string;
  created_by_user_id: string;
  archived_ts: number | null;
  created_ts: number;
  updated_ts: number;
};

export type DictionaryPersonAlias = {
  id: string;
  person_id: string;
  alias: string;
  alias_kind: string;
  created_by_user_id: string | null;
  created_ts: number;
};

export type DictionaryFact = {
  id: string;
  workspace_id: string;
  subject_kind: string;
  subject_id: string;
  fact_key: string;
  value_type: string;
  value_text: string | null;
  value_int: number | null;
  value_bool: boolean | null;
  value_date: string | null;
  value_json: unknown;
  unit: string | null;
  confidence: number | null;
  status: string;
  source_kind: string;
  source_user_id: string | null;
  source_note: string | null;
  created_ts: number;
  updated_ts: number;
};

export type DictionaryDocument = {
  id: string;
  workspace_id: string;
  subject_kind: string;
  subject_id: string;
  title: string;
  markdown_body: string;
  summary: string;
  last_edited_by_user_id: string | null;
  last_edited_source_kind: string;
  created_ts: number;
  updated_ts: number;
};

export type DictionaryResolvedRelationship = {
  relation_id: string;
  relation_group_key: string;
  relation_type: string;
  direction: string;
  other_person: DictionaryPerson;
};

export type DictionaryAccountLink = {
  user_id: string;
  space_id: string;
  person_id: string;
  family_workspace_id: string | null;
  friends_workspace_id: string | null;
  work_workspace_id: string | null;
  created_by_user_id: string;
  created_ts: number;
  updated_ts: number;
};

export type DictionaryWorkspaceMember = {
  workspace_id: string;
  user_id: string;
  login_username: string;
  display_name: string;
  role: 'owner' | 'editor' | 'viewer' | string;
  added_by_user_id: string | null;
  created_ts: number;
};

export type DictionaryBootstrapResponse = {
  workspaces: DictionaryWorkspace[];
  seeded: {
    family_workspace: DictionaryWorkspace;
    friends_workspace: DictionaryWorkspace;
    work_workspace: DictionaryWorkspace;
  };
  account_link: DictionaryAccountLink | null;
};

export type DictionaryWorkspaceTreeResponse = {
  workspace: DictionaryWorkspace;
  nodes: DictionaryTreeNode[];
};

export type DictionaryPersonBundle = {
  workspace: DictionaryWorkspace;
  person: DictionaryPerson;
  aliases: DictionaryPersonAlias[];
  nodes: DictionaryTreeNode[];
  facts: DictionaryFact[];
  relations: DictionaryResolvedRelationship[];
  document: DictionaryDocument | null;
};

export type CreateDictionaryWorkspaceInput = {
  title: string;
  slug?: string;
};

export type CreateDictionaryPersonInput = {
  display_name: string;
  canonical_name?: string;
  summary?: string;
  aliases?: string[];
  parent_node_id?: string;
  node_title?: string;
};

export type UpdateDictionaryPersonInput = {
  display_name: string;
  canonical_name?: string;
  summary?: string;
  aliases_to_add?: string[];
};

export type UpsertDictionaryFactInput = {
  value_type: 'text' | 'int' | 'bool' | 'date' | 'json';
  value_text?: string | null;
  value_int?: number | null;
  value_bool?: boolean | null;
  value_date?: string | null;
  value_json?: unknown;
  unit?: string | null;
  confidence?: number | null;
  source_note?: string | null;
};

export type SaveDictionaryDocumentInput = {
  title: string;
  markdown_body: string;
  edit_note?: string | null;
};

export type UpsertDictionaryRelationshipInput = {
  from_person_id: string;
  to_person_id: string;
  relation_type: string;
  inverse_relation_type: string;
  source_note?: string | null;
};

export type PutDictionaryAccountLinkInput = {
  person_id: string;
  family_workspace_id: string;
  friends_workspace_id?: string;
  work_workspace_id?: string;
};

export type UpsertDictionaryWorkspaceMemberInput = {
  login_username: string;
  role: 'owner' | 'editor' | 'viewer';
};

export type AttachExistingDictionaryPersonInput = {
  person_id: string;
  parent_node_id?: string;
  node_title?: string;
  as_shortcut?: boolean;
};

export async function bootstrapDictionary() {
  return apiJson<DictionaryBootstrapResponse>('/dictionary/bootstrap', {
    method: 'POST',
  });
}

export async function listDictionaryWorkspaces() {
  return apiJson<DictionaryWorkspace[]>('/dictionary/workspaces');
}

export async function createDictionaryWorkspace(input: CreateDictionaryWorkspaceInput) {
  return apiJson<DictionaryWorkspace>('/dictionary/workspaces', {
    method: 'POST',
    body: JSON.stringify(input),
  });
}

export async function getDictionaryTreeByRoot(params: {
  workspace_id?: string;
  root?: 'family' | 'friends' | 'work';
}) {
  const query = new URLSearchParams();
  if (params.workspace_id) {
    query.set('workspace_id', params.workspace_id);
  }
  if (params.root) {
    query.set('root', params.root);
  }
  return apiJson<DictionaryWorkspaceTreeResponse>(
    `/dictionary/tree${query.size ? `?${query.toString()}` : ''}`,
  );
}

export async function getDictionaryWorkspaceTree(workspaceId: string) {
  return apiJson<DictionaryWorkspaceTreeResponse>(
    `/dictionary/workspaces/${encodeURIComponent(workspaceId)}/tree`,
  );
}

export async function listDictionaryPeople(workspaceId: string, query?: string, limit?: number) {
  const params = new URLSearchParams();
  if (query && query.trim()) {
    params.set('q', query.trim());
  }
  if (typeof limit === 'number') {
    params.set('limit', String(limit));
  }
  return apiJson<DictionaryPerson[]>(
    `/dictionary/workspaces/${encodeURIComponent(workspaceId)}/people${
      params.size ? `?${params.toString()}` : ''
    }`,
  );
}

export async function attachExistingDictionaryPerson(
  workspaceId: string,
  input: AttachExistingDictionaryPersonInput,
) {
  return apiJson<DictionaryPersonBundle>(
    `/dictionary/workspaces/${encodeURIComponent(workspaceId)}/people/attach`,
    {
      method: 'POST',
      body: JSON.stringify(input),
    },
  );
}

export async function createDictionaryPerson(
  workspaceId: string,
  input: CreateDictionaryPersonInput,
) {
  return apiJson<DictionaryPersonBundle>(
    `/dictionary/workspaces/${encodeURIComponent(workspaceId)}/people`,
    {
      method: 'POST',
      body: JSON.stringify(input),
    },
  );
}

export async function getDictionaryPersonBundle(workspaceId: string, personId: string) {
  return apiJson<DictionaryPersonBundle>(
    `/dictionary/workspaces/${encodeURIComponent(workspaceId)}/people/${encodeURIComponent(personId)}`,
  );
}

export async function updateDictionaryPerson(
  workspaceId: string,
  personId: string,
  input: UpdateDictionaryPersonInput,
) {
  return apiJson<DictionaryPersonBundle>(
    `/dictionary/workspaces/${encodeURIComponent(workspaceId)}/people/${encodeURIComponent(personId)}`,
    {
      method: 'PATCH',
      body: JSON.stringify(input),
    },
  );
}

export async function deleteDictionaryPerson(workspaceId: string, personId: string) {
  return apiJson<{ deleted: boolean }>(
    `/dictionary/workspaces/${encodeURIComponent(workspaceId)}/people/${encodeURIComponent(personId)}`,
    {
      method: 'DELETE',
    },
  );
}

export async function upsertDictionaryFact(
  workspaceId: string,
  personId: string,
  factKey: string,
  input: UpsertDictionaryFactInput,
) {
  return apiJson<DictionaryFact>(
    `/dictionary/workspaces/${encodeURIComponent(workspaceId)}/people/${encodeURIComponent(personId)}/facts/${encodeURIComponent(factKey)}`,
    {
      method: 'PUT',
      body: JSON.stringify(input),
    },
  );
}

export async function getDictionaryPersonDocument(workspaceId: string, personId: string) {
  return apiJson<DictionaryDocument | null>(
    `/dictionary/workspaces/${encodeURIComponent(workspaceId)}/people/${encodeURIComponent(personId)}/document`,
  );
}

export async function saveDictionaryPersonDocument(
  workspaceId: string,
  personId: string,
  input: SaveDictionaryDocumentInput,
) {
  return apiJson<DictionaryDocument>(
    `/dictionary/workspaces/${encodeURIComponent(workspaceId)}/people/${encodeURIComponent(personId)}/document`,
    {
      method: 'PATCH',
      body: JSON.stringify(input),
    },
  );
}

export async function listDictionaryRelationships(workspaceId: string, personId: string) {
  const query = new URLSearchParams({ person_id: personId });
  return apiJson<DictionaryResolvedRelationship[]>(
    `/dictionary/workspaces/${encodeURIComponent(workspaceId)}/relationships?${query.toString()}`,
  );
}

export async function upsertDictionaryRelationship(
  workspaceId: string,
  input: UpsertDictionaryRelationshipInput,
) {
  return apiJson<DictionaryResolvedRelationship[]>(
    `/dictionary/workspaces/${encodeURIComponent(workspaceId)}/relationships`,
    {
      method: 'POST',
      body: JSON.stringify(input),
    },
  );
}

export async function updateDictionaryRelationship(
  workspaceId: string,
  relationId: string,
  input: UpsertDictionaryRelationshipInput,
) {
  return apiJson<DictionaryResolvedRelationship[]>(
    `/dictionary/workspaces/${encodeURIComponent(workspaceId)}/relationships/${encodeURIComponent(relationId)}`,
    {
      method: 'PATCH',
      body: JSON.stringify(input),
    },
  );
}

export async function deleteDictionaryRelationship(workspaceId: string, relationId: string) {
  return apiJson<{ deleted: boolean }>(
    `/dictionary/workspaces/${encodeURIComponent(workspaceId)}/relationships/${encodeURIComponent(relationId)}`,
    {
      method: 'DELETE',
    },
  );
}

export async function getMyDictionaryAccountLink() {
  return apiJson<DictionaryAccountLink | null>('/dictionary/account-link/me');
}

export async function putMyDictionaryAccountLink(input: PutDictionaryAccountLinkInput) {
  return apiJson<DictionaryAccountLink>('/dictionary/account-link/me', {
    method: 'PUT',
    body: JSON.stringify(input),
  });
}

export async function listDictionaryWorkspaceMembers(workspaceId: string) {
  return apiJson<DictionaryWorkspaceMember[]>(
    `/dictionary/workspaces/${encodeURIComponent(workspaceId)}/members`,
  );
}

export async function upsertDictionaryWorkspaceMember(
  workspaceId: string,
  input: UpsertDictionaryWorkspaceMemberInput,
) {
  return apiJson<DictionaryWorkspaceMember[]>(
    `/dictionary/workspaces/${encodeURIComponent(workspaceId)}/members`,
    {
      method: 'POST',
      body: JSON.stringify(input),
    },
  );
}

export async function deleteDictionaryWorkspaceMember(workspaceId: string, userId: string) {
  return apiJson<{ deleted: boolean }>(
    `/dictionary/workspaces/${encodeURIComponent(workspaceId)}/members/${encodeURIComponent(userId)}`,
    {
      method: 'DELETE',
    },
  );
}

'use client';

import { FormEvent, useEffect, useState } from 'react';
import { useRouter } from 'next/navigation';

import { useAuth } from '@/lib/auth';
import {
  bootstrapDictionary,
  createDictionaryPerson,
  createDictionaryWorkspace,
  deleteDictionaryPerson,
  deleteDictionaryRelationship,
  DictionaryBootstrapResponse,
  DictionaryFact,
  DictionaryPerson,
  DictionaryPersonBundle,
  DictionaryRelationship,
  DictionaryTreeNode,
  DictionaryWorkspace,
  getDictionaryPersonBundle,
  getDictionaryWorkspaceTree,
  listDictionaryPeople,
  putMyDictionaryAccountLink,
  saveDictionaryPersonDocument,
  updateDictionaryPerson,
  upsertDictionaryFact,
  upsertDictionaryRelationship,
} from '@/lib/dictionaryApi';

type RootKey = 'family' | 'friends' | 'work' | 'custom';

type DraftStatus = {
  tone: 'idle' | 'success' | 'error';
  message: string;
};

type PersonDraft = {
  displayName: string;
  canonicalName: string;
  summary: string;
  aliasInput: string;
};

type QuickFactDraft = {
  birthday: string;
  hobbies: string;
  factKey: string;
  factType: 'text' | 'int' | 'bool' | 'date' | 'json';
  factValue: string;
};

type RelationshipDraft = {
  toPersonId: string;
  relationType: string;
  inverseRelationType: string;
  sourceNote: string;
};

const ROOT_ORDER: RootKey[] = ['family', 'friends', 'work', 'custom'];
const DEFAULT_RELATIONS = [
  ['mother_of', 'child_of'],
  ['father_of', 'child_of'],
  ['sibling_of', 'sibling_of'],
  ['spouse_of', 'spouse_of'],
  ['friend_of', 'friend_of'],
  ['coworker_of', 'coworker_of'],
  ['manager_of', 'reports_to'],
];

function workspaceRootKey(workspace: DictionaryWorkspace): RootKey {
  switch (workspace.workspace_kind) {
    case 'family_shared':
      return 'family';
    case 'friends_private':
      return 'friends';
    case 'work_private':
      return 'work';
    default:
      return 'custom';
  }
}

function workspaceRootLabel(root: RootKey) {
  switch (root) {
    case 'family':
      return 'Family';
    case 'friends':
      return 'Friends';
    case 'work':
      return 'Work';
    default:
      return 'Custom';
  }
}

function workspaceKindMeta(workspace: DictionaryWorkspace) {
  switch (workspace.workspace_kind) {
    case 'family_shared':
      return 'Shared household space';
    case 'friends_private':
      return 'Private friend graph';
    case 'work_private':
      return 'Private work graph';
    default:
      return 'Custom workspace';
  }
}

function timestampLabel(value: number | null | undefined) {
  if (!value) return 'Unknown';
  return new Date(value).toLocaleString();
}

function factByKey(facts: DictionaryFact[], factKey: string) {
  return facts.find((fact) => fact.fact_key === factKey);
}

function factToEditableString(fact: DictionaryFact | undefined) {
  if (!fact) return '';
  if (fact.value_type === 'date') {
    return fact.value_date ?? '';
  }
  if (fact.value_type === 'text') {
    return fact.value_text ?? '';
  }
  if (fact.value_type === 'int') {
    return fact.value_int == null ? '' : String(fact.value_int);
  }
  if (fact.value_type === 'bool') {
    return fact.value_bool ? 'true' : 'false';
  }
  if (fact.value_type === 'json') {
    if (Array.isArray(fact.value_json)) {
      return fact.value_json.join(', ');
    }
    if (fact.value_json == null) {
      return '';
    }
    return JSON.stringify(fact.value_json);
  }
  return '';
}

function factSummaryLabel(fact: DictionaryFact) {
  switch (fact.value_type) {
    case 'date':
      return fact.value_date ?? 'No date';
    case 'text':
      return fact.value_text ?? 'No text';
    case 'int':
      return fact.value_int == null ? 'No value' : String(fact.value_int);
    case 'bool':
      return fact.value_bool ? 'True' : 'False';
    case 'json':
      return factToEditableString(fact) || 'No structured value';
    default:
      return 'No value';
  }
}

function relationshipTone(relation: string) {
  if (relation.includes('mother') || relation.includes('father') || relation.includes('child')) {
    return 'text-rose-100/90';
  }
  if (relation.includes('coworker') || relation.includes('manager') || relation.includes('reports')) {
    return 'text-sky-100/85';
  }
  return 'text-white/80';
}

function personMatchesQuery(person: DictionaryPerson, query: string) {
  const normalized = query.trim().toLowerCase();
  if (!normalized) return true;
  return (
    person.display_name.toLowerCase().includes(normalized) ||
    person.canonical_name.toLowerCase().includes(normalized) ||
    (person.summary ?? '').toLowerCase().includes(normalized)
  );
}

function sortNodes(nodes: DictionaryTreeNode[]) {
  return [...nodes].sort((left, right) => {
    if (left.sort_order !== right.sort_order) {
      return left.sort_order - right.sort_order;
    }
    return left.title.localeCompare(right.title);
  });
}

function buildChildrenByParent(nodes: DictionaryTreeNode[]) {
  const map = new Map<string | null, DictionaryTreeNode[]>();
  for (const node of sortNodes(nodes)) {
    const key = node.parent_node_id ?? null;
    const existing = map.get(key);
    if (existing) {
      existing.push(node);
    } else {
      map.set(key, [node]);
    }
  }
  return map;
}

function workspaceRootsFromBootstrap(bootstrap: DictionaryBootstrapResponse | null) {
  if (!bootstrap) return [];
  const buckets = new Map<RootKey, DictionaryWorkspace[]>();
  for (const root of ROOT_ORDER) {
    buckets.set(root, []);
  }
  for (const workspace of bootstrap.workspaces) {
    const root = workspaceRootKey(workspace);
    buckets.get(root)?.push(workspace);
  }
  return ROOT_ORDER.flatMap((root) =>
    (buckets.get(root) ?? []).sort((left, right) => left.title.localeCompare(right.title)),
  );
}

function loadInitialExpanded(nodes: DictionaryTreeNode[]) {
  const next = new Set<string>();
  for (const node of nodes) {
    if (node.node_kind !== 'person') {
      next.add(node.id);
    }
  }
  return next;
}

export default function DictionaryPage() {
  const router = useRouter();
  const { me, loading } = useAuth();
  const [bootstrap, setBootstrap] = useState<DictionaryBootstrapResponse | null>(null);
  const [selectedWorkspaceId, setSelectedWorkspaceId] = useState<string | null>(null);
  const [selectedPersonId, setSelectedPersonId] = useState<string | null>(null);
  const [workspaceTree, setWorkspaceTree] = useState<DictionaryTreeNode[]>([]);
  const [workspacePeople, setWorkspacePeople] = useState<DictionaryPerson[]>([]);
  const [selectedBundle, setSelectedBundle] = useState<DictionaryPersonBundle | null>(null);
  const [expandedNodes, setExpandedNodes] = useState<Set<string>>(new Set());
  const [loadingShell, setLoadingShell] = useState(true);
  const [loadingWorkspace, setLoadingWorkspace] = useState(false);
  const [loadingPerson, setLoadingPerson] = useState(false);
  const [saving, setSaving] = useState(false);
  const [status, setStatus] = useState<DraftStatus>({ tone: 'idle', message: '' });
  const [treeSearch, setTreeSearch] = useState('');
  const [workspaceTitleDraft, setWorkspaceTitleDraft] = useState('');
  const [workspaceSlugDraft, setWorkspaceSlugDraft] = useState('');
  const [personDraft, setPersonDraft] = useState<PersonDraft>({
    displayName: '',
    canonicalName: '',
    summary: '',
    aliasInput: '',
  });
  const [createPersonDraft, setCreatePersonDraft] = useState<CreatePersonRequestState>({
    displayName: '',
    summary: '',
    aliases: '',
    nodeTitle: '',
    parentNodeId: '',
  });
  const [quickFacts, setQuickFacts] = useState<QuickFactDraft>({
    birthday: '',
    hobbies: '',
    factKey: '',
    factType: 'text',
    factValue: '',
  });
  const [relationshipDraft, setRelationshipDraft] = useState<RelationshipDraft>({
    toPersonId: '',
    relationType: 'friend_of',
    inverseRelationType: 'friend_of',
    sourceNote: '',
  });
  const [documentDraft, setDocumentDraft] = useState('');
  const [documentTitleDraft, setDocumentTitleDraft] = useState('Profile');

  const visibleWorkspaces = workspaceRootsFromBootstrap(bootstrap);
  const selectedWorkspace =
    visibleWorkspaces.find((workspace) => workspace.id === selectedWorkspaceId) ?? null;
  const activeAccountLink = bootstrap?.account_link ?? null;
  const childrenByParent = buildChildrenByParent(workspaceTree);
  const peopleById = new Map(workspacePeople.map((person) => [person.id, person]));
  const searchMatches = treeSearch.trim()
    ? workspacePeople.filter((person) => personMatchesQuery(person, treeSearch)).slice(0, 8)
    : [];

  useEffect(() => {
    if (!loading && !me) {
      router.replace('/login');
    }
  }, [loading, me, router]);

  useEffect(() => {
    if (!me) return;
    let cancelled = false;
    const run = async () => {
      setLoadingShell(true);
      try {
        const data = await bootstrapDictionary();
        if (cancelled) return;
        setBootstrap(data);
        const firstWorkspace =
          data.seeded.family_workspace?.id ??
          data.workspaces.find((workspace) => workspace.workspace_kind === 'family_shared')?.id ??
          data.workspaces[0]?.id ??
          null;
        setSelectedWorkspaceId((current) => current ?? firstWorkspace);
      } catch (error) {
        if (cancelled) return;
        setStatus({
          tone: 'error',
          message: error instanceof Error ? error.message : 'Failed to load Dictionary.',
        });
      } finally {
        if (!cancelled) {
          setLoadingShell(false);
        }
      }
    };
    void run();
    return () => {
      cancelled = true;
    };
  }, [me]);

  useEffect(() => {
    if (!selectedWorkspaceId || !me) return;
    let cancelled = false;
    const run = async () => {
      setLoadingWorkspace(true);
      try {
        const [treeResponse, people] = await Promise.all([
          getDictionaryWorkspaceTree(selectedWorkspaceId),
          listDictionaryPeople(selectedWorkspaceId),
        ]);
        if (cancelled) return;
        setWorkspaceTree(treeResponse.nodes);
        setWorkspacePeople(people);
        setExpandedNodes(loadInitialExpanded(treeResponse.nodes));
        if (!people.some((person) => person.id === selectedPersonId)) {
          const firstVisiblePerson = treeResponse.nodes.find((node) => node.node_kind === 'person' && node.person_id);
          setSelectedPersonId(firstVisiblePerson?.person_id ?? null);
        }
      } catch (error) {
        if (cancelled) return;
        setStatus({
          tone: 'error',
          message: error instanceof Error ? error.message : 'Failed to load Dictionary workspace.',
        });
      } finally {
        if (!cancelled) {
          setLoadingWorkspace(false);
        }
      }
    };
    void run();
    return () => {
      cancelled = true;
    };
  }, [me, selectedWorkspaceId]);

  useEffect(() => {
    if (!selectedWorkspaceId || !selectedPersonId) {
      setSelectedBundle(null);
      return;
    }
    let cancelled = false;
    const run = async () => {
      setLoadingPerson(true);
      try {
        const bundle = await getDictionaryPersonBundle(selectedWorkspaceId, selectedPersonId);
        if (cancelled) return;
        setSelectedBundle(bundle);
        setPersonDraft({
          displayName: bundle.person.display_name,
          canonicalName: bundle.person.canonical_name,
          summary: bundle.person.summary ?? '',
          aliasInput: '',
        });
        setQuickFacts({
          birthday: factByKey(bundle.facts, 'birthday')?.value_date ?? '',
          hobbies: factToEditableString(factByKey(bundle.facts, 'hobbies')),
          factKey: '',
          factType: 'text',
          factValue: '',
        });
        setRelationshipDraft((current) => ({
          ...current,
          toPersonId: '',
          sourceNote: '',
        }));
        setDocumentTitleDraft(bundle.document?.title ?? `${bundle.person.display_name} profile`);
        setDocumentDraft(bundle.document?.markdown_body ?? '');
      } catch (error) {
        if (cancelled) return;
        setStatus({
          tone: 'error',
          message: error instanceof Error ? error.message : 'Failed to load person details.',
        });
      } finally {
        if (!cancelled) {
          setLoadingPerson(false);
        }
      }
    };
    void run();
    return () => {
      cancelled = true;
    };
  }, [selectedPersonId, selectedWorkspaceId]);

  function setNotice(tone: DraftStatus['tone'], message: string) {
    setStatus({ tone, message });
  }

  function toggleExpanded(nodeId: string) {
    setExpandedNodes((current) => {
      const next = new Set(current);
      if (next.has(nodeId)) {
        next.delete(nodeId);
      } else {
        next.add(nodeId);
      }
      return next;
    });
  }

  async function refreshWorkspaceBundle(nextPersonId?: string | null) {
    if (!selectedWorkspaceId) return;
    const [treeResponse, people] = await Promise.all([
      getDictionaryWorkspaceTree(selectedWorkspaceId),
      listDictionaryPeople(selectedWorkspaceId),
    ]);
    setWorkspaceTree(treeResponse.nodes);
    setWorkspacePeople(people);
    setExpandedNodes((current) => {
      if (current.size > 0) return current;
      return loadInitialExpanded(treeResponse.nodes);
    });
    if (nextPersonId !== undefined) {
      setSelectedPersonId(nextPersonId);
    }
  }

  async function handleCreateWorkspace(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!workspaceTitleDraft.trim()) {
      setNotice('error', 'Workspace title is required.');
      return;
    }
    setSaving(true);
    try {
      const workspace = await createDictionaryWorkspace({
        title: workspaceTitleDraft.trim(),
        slug: workspaceSlugDraft.trim() || undefined,
      });
      const refreshed = await bootstrapDictionary();
      setBootstrap(refreshed);
      setSelectedWorkspaceId(workspace.id);
      setWorkspaceTitleDraft('');
      setWorkspaceSlugDraft('');
      setNotice('success', `Created ${workspace.title}.`);
    } catch (error) {
      setNotice('error', error instanceof Error ? error.message : 'Failed to create workspace.');
    } finally {
      setSaving(false);
    }
  }

  async function handleCreatePerson(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!selectedWorkspaceId) return;
    if (!createPersonDraft.displayName.trim()) {
      setNotice('error', 'Display name is required.');
      return;
    }
    setSaving(true);
    try {
      const bundle = await createDictionaryPerson(selectedWorkspaceId, {
        display_name: createPersonDraft.displayName.trim(),
        summary: createPersonDraft.summary.trim() || undefined,
        aliases: createPersonDraft.aliases
          .split(',')
          .map((alias) => alias.trim())
          .filter(Boolean),
        node_title: createPersonDraft.nodeTitle.trim() || undefined,
        parent_node_id: createPersonDraft.parentNodeId || undefined,
      });
      await refreshWorkspaceBundle(bundle.person.id);
      setSelectedBundle(bundle);
      setCreatePersonDraft({
        displayName: '',
        summary: '',
        aliases: '',
        nodeTitle: '',
        parentNodeId: '',
      });
      setNotice('success', `Added ${bundle.person.display_name}.`);
    } catch (error) {
      setNotice('error', error instanceof Error ? error.message : 'Failed to add person.');
    } finally {
      setSaving(false);
    }
  }

  async function handleSavePerson(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!selectedWorkspaceId || !selectedBundle) return;
    setSaving(true);
    try {
      const bundle = await updateDictionaryPerson(selectedWorkspaceId, selectedBundle.person.id, {
        display_name: personDraft.displayName.trim(),
        canonical_name: personDraft.canonicalName.trim() || undefined,
        summary: personDraft.summary.trim() || undefined,
        aliases_to_add: personDraft.aliasInput
          .split(',')
          .map((alias) => alias.trim())
          .filter(Boolean),
      });
      setSelectedBundle(bundle);
      setPersonDraft((current) => ({ ...current, aliasInput: '' }));
      await refreshWorkspaceBundle(bundle.person.id);
      setNotice('success', `Updated ${bundle.person.display_name}.`);
    } catch (error) {
      setNotice('error', error instanceof Error ? error.message : 'Failed to update person.');
    } finally {
      setSaving(false);
    }
  }

  async function handleDeletePerson() {
    if (!selectedWorkspaceId || !selectedBundle) return;
    if (!window.confirm(`Delete ${selectedBundle.person.display_name} from this workspace?`)) {
      return;
    }
    setSaving(true);
    try {
      await deleteDictionaryPerson(selectedWorkspaceId, selectedBundle.person.id);
      await refreshWorkspaceBundle(null);
      setSelectedBundle(null);
      setNotice('success', `${selectedBundle.person.display_name} was removed from this workspace.`);
    } catch (error) {
      setNotice('error', error instanceof Error ? error.message : 'Failed to delete person.');
    } finally {
      setSaving(false);
    }
  }

  async function handleSaveBirthday() {
    if (!selectedWorkspaceId || !selectedBundle) return;
    setSaving(true);
    try {
      await upsertDictionaryFact(selectedWorkspaceId, selectedBundle.person.id, 'birthday', {
        value_type: 'date',
        value_date: quickFacts.birthday || null,
        source_note: 'Dictionary quick fact update',
      });
      const bundle = await getDictionaryPersonBundle(selectedWorkspaceId, selectedBundle.person.id);
      setSelectedBundle(bundle);
      setNotice('success', 'Birthday saved.');
    } catch (error) {
      setNotice('error', error instanceof Error ? error.message : 'Failed to save birthday.');
    } finally {
      setSaving(false);
    }
  }

  async function handleSaveHobbies() {
    if (!selectedWorkspaceId || !selectedBundle) return;
    const hobbies = quickFacts.hobbies
      .split(/[,|\n]/)
      .map((value) => value.trim())
      .filter(Boolean);
    setSaving(true);
    try {
      await upsertDictionaryFact(selectedWorkspaceId, selectedBundle.person.id, 'hobbies', {
        value_type: 'json',
        value_json: hobbies,
        source_note: 'Dictionary quick fact update',
      });
      const bundle = await getDictionaryPersonBundle(selectedWorkspaceId, selectedBundle.person.id);
      setSelectedBundle(bundle);
      setNotice('success', 'Hobbies saved.');
    } catch (error) {
      setNotice('error', error instanceof Error ? error.message : 'Failed to save hobbies.');
    } finally {
      setSaving(false);
    }
  }

  async function handleSaveGenericFact(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!selectedWorkspaceId || !selectedBundle) return;
    if (!quickFacts.factKey.trim()) {
      setNotice('error', 'Fact key is required.');
      return;
    }

    setSaving(true);
    try {
      const payload = {
        value_type: quickFacts.factType,
        source_note: 'Dictionary custom fact update',
      } as Parameters<typeof upsertDictionaryFact>[3];

      if (quickFacts.factType === 'text') {
        payload.value_text = quickFacts.factValue.trim();
      } else if (quickFacts.factType === 'int') {
        payload.value_int = quickFacts.factValue.trim()
          ? Number.parseInt(quickFacts.factValue.trim(), 10)
          : null;
      } else if (quickFacts.factType === 'bool') {
        payload.value_bool = quickFacts.factValue.trim().toLowerCase() === 'true';
      } else if (quickFacts.factType === 'date') {
        payload.value_date = quickFacts.factValue.trim() || null;
      } else {
        payload.value_json = quickFacts.factValue.trim() ? JSON.parse(quickFacts.factValue) : null;
      }

      await upsertDictionaryFact(
        selectedWorkspaceId,
        selectedBundle.person.id,
        quickFacts.factKey.trim().toLowerCase(),
        payload,
      );
      const bundle = await getDictionaryPersonBundle(selectedWorkspaceId, selectedBundle.person.id);
      setSelectedBundle(bundle);
      setQuickFacts((current) => ({ ...current, factKey: '', factValue: '' }));
      setNotice('success', 'Fact saved.');
    } catch (error) {
      setNotice('error', error instanceof Error ? error.message : 'Failed to save custom fact.');
    } finally {
      setSaving(false);
    }
  }

  async function handleSaveDocument(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!selectedWorkspaceId || !selectedBundle) return;
    setSaving(true);
    try {
      const document = await saveDictionaryPersonDocument(selectedWorkspaceId, selectedBundle.person.id, {
        title: documentTitleDraft.trim() || `${selectedBundle.person.display_name} profile`,
        markdown_body: documentDraft,
        edit_note: 'Dictionary workspace edit',
      });
      setSelectedBundle((current) =>
        current
          ? {
              ...current,
              document,
            }
          : current,
      );
      setNotice('success', 'Person document saved.');
    } catch (error) {
      setNotice('error', error instanceof Error ? error.message : 'Failed to save document.');
    } finally {
      setSaving(false);
    }
  }

  async function handleCreateRelationship(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!selectedWorkspaceId || !selectedBundle) return;
    if (!relationshipDraft.toPersonId) {
      setNotice('error', 'Choose another person first.');
      return;
    }
    setSaving(true);
    try {
      const relations = await upsertDictionaryRelationship(selectedWorkspaceId, {
        from_person_id: selectedBundle.person.id,
        to_person_id: relationshipDraft.toPersonId,
        relation_type: relationshipDraft.relationType.trim().toLowerCase(),
        inverse_relation_type: relationshipDraft.inverseRelationType.trim().toLowerCase(),
        source_note: relationshipDraft.sourceNote.trim() || undefined,
      });
      setSelectedBundle((current) =>
        current
          ? {
              ...current,
              relations,
            }
          : current,
      );
      setRelationshipDraft({
        toPersonId: '',
        relationType: 'friend_of',
        inverseRelationType: 'friend_of',
        sourceNote: '',
      });
      setNotice('success', 'Relationship saved.');
    } catch (error) {
      setNotice('error', error instanceof Error ? error.message : 'Failed to save relationship.');
    } finally {
      setSaving(false);
    }
  }

  async function handleDeleteRelationship(relation: DictionaryRelationship) {
    if (!selectedWorkspaceId || !selectedBundle) return;
    if (!window.confirm(`Remove the ${relation.relation_type} relationship?`)) {
      return;
    }
    setSaving(true);
    try {
      await deleteDictionaryRelationship(selectedWorkspaceId, relation.id);
      const bundle = await getDictionaryPersonBundle(selectedWorkspaceId, selectedBundle.person.id);
      setSelectedBundle(bundle);
      setNotice('success', 'Relationship removed.');
    } catch (error) {
      setNotice('error', error instanceof Error ? error.message : 'Failed to remove relationship.');
    } finally {
      setSaving(false);
    }
  }

  async function handleLinkAccount() {
    if (!selectedBundle || !bootstrap) return;
    const familyWorkspaceId = bootstrap.seeded.family_workspace.id;
    setSaving(true);
    try {
      const accountLink = await putMyDictionaryAccountLink({
        person_id: selectedBundle.person.id,
        family_workspace_id: familyWorkspaceId,
        friends_workspace_id: bootstrap.seeded.friends_workspace.id,
        work_workspace_id: bootstrap.seeded.work_workspace.id,
      });
      setBootstrap((current) =>
        current
          ? {
              ...current,
              account_link: accountLink,
            }
          : current,
      );
      setNotice('success', `${selectedBundle.person.display_name} is now linked to your Rustyfin account.`);
    } catch (error) {
      setNotice('error', error instanceof Error ? error.message : 'Failed to link your account.');
    } finally {
      setSaving(false);
    }
  }

  if (loading || loadingShell) {
    return (
      <div className="rf-flat-empty animate-rise">
        <p className="text-sm muted">Loading Dictionary...</p>
      </div>
    );
  }

  if (!me) {
    return (
      <div className="rf-flat-empty animate-rise">
        <p className="text-sm muted">Redirecting to login...</p>
      </div>
    );
  }

  const linkedPersonId = activeAccountLink?.person_id ?? null;
  const isSelectedPersonLinked = selectedBundle?.person.id === linkedPersonId;
  const canLinkSelectedPerson =
    !!selectedBundle && bootstrap?.seeded.family_workspace.space_id === selectedBundle.person.space_id;

  return (
    <div className="animate-rise rf-flat-page space-y-6">
      <section className="rf-flat-section space-y-5">
        <header className="flex flex-col gap-5 border-b border-[var(--border-subtle)] pb-4 lg:flex-row lg:items-end lg:justify-between">
          <div className="max-w-3xl space-y-2">
            <p className="text-xs uppercase tracking-[0.18em] text-white/55">Social</p>
            <h1 className="text-2xl font-semibold sm:text-3xl">Human Dictionary</h1>
            <p className="max-w-3xl text-sm muted">
              Keep family, friends, and work relationships in a tree-first workspace that still stores facts,
              documents, and relationship links as structured data.
            </p>
            {status.message ? (
              <p className={status.tone === 'error' ? 'text-sm text-rose-200/90' : 'text-sm text-white/82'}>
                {status.message}
              </p>
            ) : null}
          </div>

          <form className="grid gap-2 sm:grid-cols-[minmax(0,1fr)_12rem_auto]" onSubmit={handleCreateWorkspace}>
            <input
              value={workspaceTitleDraft}
              onChange={(event) => setWorkspaceTitleDraft(event.target.value)}
              placeholder="New workspace title"
              className="h-11 rounded-[1rem] border border-[var(--border-subtle)] bg-transparent px-4 text-sm text-white outline-none transition focus:border-white/28"
            />
            <input
              value={workspaceSlugDraft}
              onChange={(event) => setWorkspaceSlugDraft(event.target.value)}
              placeholder="slug-optional"
              className="h-11 rounded-[1rem] border border-[var(--border-subtle)] bg-transparent px-4 text-sm text-white outline-none transition focus:border-white/28"
            />
            <button className="btn-secondary px-4 py-2 text-sm disabled:opacity-60" disabled={saving}>
              Add workspace
            </button>
          </form>
        </header>

        <div className="grid gap-3 lg:grid-cols-[16rem_minmax(0,1fr)]">
          <div className="space-y-2">
            <p className="text-xs uppercase tracking-[0.16em] text-white/45">Roots</p>
            <div className="space-y-2">
              {visibleWorkspaces.map((workspace) => {
                const active = workspace.id === selectedWorkspaceId;
                return (
                  <button
                    key={workspace.id}
                    type="button"
                    onClick={() => {
                      setSelectedWorkspaceId(workspace.id);
                      setSelectedPersonId(null);
                    }}
                    className={`w-full rounded-[1.1rem] px-4 py-3 text-left transition ${
                      active
                        ? 'border border-white/18 bg-white/[0.06]'
                        : 'border border-transparent bg-transparent hover:border-white/10 hover:bg-white/[0.03]'
                    }`}
                  >
                    <div className="flex items-center justify-between gap-3">
                      <span className="text-sm font-semibold text-white/92">{workspace.title}</span>
                      <span className="text-[0.68rem] uppercase tracking-[0.18em] text-white/40">
                        {workspaceRootLabel(workspaceRootKey(workspace))}
                      </span>
                    </div>
                    <p className="mt-1 text-xs muted">{workspaceKindMeta(workspace)}</p>
                  </button>
                );
              })}
            </div>
          </div>

          <div className="grid gap-5 xl:grid-cols-[minmax(20rem,25rem)_minmax(0,1fr)]">
            <section className="space-y-4 border-t border-[var(--border-subtle)] pt-4">
              <div className="space-y-1">
                <div className="flex items-center justify-between gap-3">
                  <h2 className="text-lg font-semibold text-white/94">
                    {selectedWorkspace?.title ?? 'Workspace'}
                  </h2>
                  {loadingWorkspace ? <span className="text-xs muted">Refreshing…</span> : null}
                </div>
                <p className="text-sm muted">
                  Expand branches, choose a person, and keep relationship context tied to the current workspace.
                </p>
              </div>

              <div className="space-y-2">
                <input
                  value={treeSearch}
                  onChange={(event) => setTreeSearch(event.target.value)}
                  placeholder="Search people in this workspace"
                  className="h-11 w-full rounded-[1rem] border border-[var(--border-subtle)] bg-transparent px-4 text-sm text-white outline-none transition focus:border-white/28"
                />
                {treeSearch.trim() ? (
                  <div className="space-y-1 rounded-[1rem] border border-[var(--border-subtle)] bg-white/[0.02] p-2">
                    {searchMatches.length > 0 ? (
                      searchMatches.map((person) => (
                        <button
                          key={person.id}
                          type="button"
                          onClick={() => setSelectedPersonId(person.id)}
                          className="flex w-full items-center justify-between rounded-[0.85rem] px-3 py-2 text-left transition hover:bg-white/[0.04]"
                        >
                          <span className="text-sm text-white/90">{person.display_name}</span>
                          <span className="text-xs muted">{person.summary || 'Profile'}</span>
                        </button>
                      ))
                    ) : (
                      <p className="px-3 py-2 text-sm muted">No visible people match that search.</p>
                    )}
                  </div>
                ) : null}
              </div>

              <div className="space-y-2 border-t border-[var(--border-subtle)] pt-4">
                <p className="text-xs uppercase tracking-[0.16em] text-white/45">Tree</p>
                <div className="space-y-1">
                  {(childrenByParent.get(null) ?? []).map((node) => (
                    <DictionaryTreeBranch
                      key={node.id}
                      node={node}
                      depth={0}
                      selectedPersonId={selectedPersonId}
                      expandedNodes={expandedNodes}
                      childrenByParent={childrenByParent}
                      peopleById={peopleById}
                      onSelectPerson={setSelectedPersonId}
                      onToggleExpanded={toggleExpanded}
                    />
                  ))}
                </div>
              </div>

              <form className="space-y-3 border-t border-[var(--border-subtle)] pt-4" onSubmit={handleCreatePerson}>
                <div className="flex items-center justify-between gap-3">
                  <h3 className="text-sm font-semibold text-white/90">Add person</h3>
                  <span className="text-xs muted">Tree-first v1</span>
                </div>
                <input
                  value={createPersonDraft.displayName}
                  onChange={(event) =>
                    setCreatePersonDraft((current) => ({ ...current, displayName: event.target.value }))
                  }
                  placeholder="Display name"
                  className="h-11 w-full rounded-[1rem] border border-[var(--border-subtle)] bg-transparent px-4 text-sm text-white outline-none transition focus:border-white/28"
                />
                <textarea
                  value={createPersonDraft.summary}
                  onChange={(event) =>
                    setCreatePersonDraft((current) => ({ ...current, summary: event.target.value }))
                  }
                  placeholder="Short summary"
                  rows={3}
                  className="w-full rounded-[1rem] border border-[var(--border-subtle)] bg-transparent px-4 py-3 text-sm text-white outline-none transition focus:border-white/28"
                />
                <div className="grid gap-3 sm:grid-cols-2">
                  <input
                    value={createPersonDraft.aliases}
                    onChange={(event) =>
                      setCreatePersonDraft((current) => ({ ...current, aliases: event.target.value }))
                    }
                    placeholder="Aliases, comma separated"
                    className="h-11 rounded-[1rem] border border-[var(--border-subtle)] bg-transparent px-4 text-sm text-white outline-none transition focus:border-white/28"
                  />
                  <input
                    value={createPersonDraft.nodeTitle}
                    onChange={(event) =>
                      setCreatePersonDraft((current) => ({ ...current, nodeTitle: event.target.value }))
                    }
                    placeholder="Optional node title"
                    className="h-11 rounded-[1rem] border border-[var(--border-subtle)] bg-transparent px-4 text-sm text-white outline-none transition focus:border-white/28"
                  />
                </div>
                <select
                  value={createPersonDraft.parentNodeId}
                  onChange={(event) =>
                    setCreatePersonDraft((current) => ({ ...current, parentNodeId: event.target.value }))
                  }
                  className="h-11 w-full rounded-[1rem] border border-[var(--border-subtle)] bg-transparent px-4 text-sm text-white outline-none transition focus:border-white/28"
                >
                  <option value="">Auto place in first group</option>
                  {workspaceTree
                    .filter((node) => node.node_kind === 'group' || node.node_kind === 'root')
                    .map((node) => (
                      <option key={node.id} value={node.id}>
                        {node.title}
                      </option>
                    ))}
                </select>
                <button className="btn-primary w-full px-5 py-2.5 text-sm disabled:opacity-60" disabled={saving}>
                  Add person
                </button>
              </form>
            </section>

            <section className="space-y-5 border-t border-[var(--border-subtle)] pt-4">
              {loadingPerson ? (
                <div className="rf-flat-empty">
                  <p className="text-sm muted">Loading person details...</p>
                </div>
              ) : selectedBundle ? (
                <>
                  <header className="flex flex-col gap-4 border-b border-[var(--border-subtle)] pb-4 lg:flex-row lg:items-start lg:justify-between">
                    <div className="space-y-2">
                      <p className="text-xs uppercase tracking-[0.16em] text-white/45">
                        {selectedBundle.workspace.title}
                      </p>
                      <h2 className="text-2xl font-semibold text-white/94">
                        {selectedBundle.person.display_name}
                      </h2>
                      <p className="max-w-3xl text-sm muted">
                        {selectedBundle.person.summary || 'No short summary yet. Use the form below to add one.'}
                      </p>
                    </div>

                    <div className="flex flex-wrap gap-2">
                      {canLinkSelectedPerson ? (
                        <button
                          type="button"
                          onClick={() => void handleLinkAccount()}
                          className="btn-secondary px-4 py-2 text-sm"
                          disabled={saving || isSelectedPersonLinked}
                        >
                          {isSelectedPersonLinked ? 'Linked to my account' : 'Link my account'}
                        </button>
                      ) : null}
                      <button
                        type="button"
                        onClick={() => void handleDeletePerson()}
                        className="btn-secondary px-4 py-2 text-sm text-rose-200/90"
                        disabled={saving}
                      >
                        Remove from workspace
                      </button>
                    </div>
                  </header>

                  <div className="grid gap-5 xl:grid-cols-[minmax(0,1.1fr)_minmax(18rem,0.9fr)]">
                    <div className="space-y-5">
                      <form className="space-y-4" onSubmit={handleSavePerson}>
                        <div className="space-y-1">
                          <h3 className="text-sm font-semibold text-white/90">Profile</h3>
                          <p className="text-sm muted">
                            Keep the person record clean, searchable, and tied to the right aliases.
                          </p>
                        </div>
                        <div className="grid gap-3 md:grid-cols-2">
                          <label className="space-y-2 text-sm text-white/86">
                            <span className="block text-xs uppercase tracking-[0.14em] text-white/45">
                              Display name
                            </span>
                            <input
                              value={personDraft.displayName}
                              onChange={(event) =>
                                setPersonDraft((current) => ({ ...current, displayName: event.target.value }))
                              }
                              className="h-11 w-full rounded-[1rem] border border-[var(--border-subtle)] bg-transparent px-4 text-sm text-white outline-none transition focus:border-white/28"
                            />
                          </label>
                          <label className="space-y-2 text-sm text-white/86">
                            <span className="block text-xs uppercase tracking-[0.14em] text-white/45">
                              Canonical name
                            </span>
                            <input
                              value={personDraft.canonicalName}
                              onChange={(event) =>
                                setPersonDraft((current) => ({ ...current, canonicalName: event.target.value }))
                              }
                              className="h-11 w-full rounded-[1rem] border border-[var(--border-subtle)] bg-transparent px-4 text-sm text-white outline-none transition focus:border-white/28"
                            />
                          </label>
                        </div>

                        <label className="space-y-2 text-sm text-white/86">
                          <span className="block text-xs uppercase tracking-[0.14em] text-white/45">Summary</span>
                          <textarea
                            value={personDraft.summary}
                            onChange={(event) =>
                              setPersonDraft((current) => ({ ...current, summary: event.target.value }))
                            }
                            rows={4}
                            className="w-full rounded-[1rem] border border-[var(--border-subtle)] bg-transparent px-4 py-3 text-sm text-white outline-none transition focus:border-white/28"
                          />
                        </label>

                        <label className="space-y-2 text-sm text-white/86">
                          <span className="block text-xs uppercase tracking-[0.14em] text-white/45">
                            Add aliases
                          </span>
                          <input
                            value={personDraft.aliasInput}
                            onChange={(event) =>
                              setPersonDraft((current) => ({ ...current, aliasInput: event.target.value }))
                            }
                            placeholder="Mum, Mam, Mary"
                            className="h-11 w-full rounded-[1rem] border border-[var(--border-subtle)] bg-transparent px-4 text-sm text-white outline-none transition focus:border-white/28"
                          />
                        </label>

                        <div className="flex flex-wrap gap-2">
                          {selectedBundle.aliases.map((alias) => (
                            <span
                              key={alias.id}
                              className="rounded-full border border-white/10 px-3 py-1 text-xs text-white/72"
                            >
                              {alias.alias}
                            </span>
                          ))}
                        </div>

                        <button className="btn-primary px-5 py-2.5 text-sm disabled:opacity-60" disabled={saving}>
                          Save profile
                        </button>
                      </form>

                      <section className="space-y-4 border-t border-[var(--border-subtle)] pt-4">
                        <div className="space-y-1">
                          <h3 className="text-sm font-semibold text-white/90">Facts</h3>
                          <p className="text-sm muted">
                            Quick structured fields for birthdays, hobbies, and other stable facts.
                          </p>
                        </div>

                        <div className="grid gap-4 lg:grid-cols-2">
                          <div className="space-y-3">
                            <label className="space-y-2 text-sm text-white/86">
                              <span className="block text-xs uppercase tracking-[0.14em] text-white/45">
                                Birthday
                              </span>
                              <input
                                type="date"
                                value={quickFacts.birthday}
                                onChange={(event) =>
                                  setQuickFacts((current) => ({ ...current, birthday: event.target.value }))
                                }
                                className="h-11 w-full rounded-[1rem] border border-[var(--border-subtle)] bg-transparent px-4 text-sm text-white outline-none transition focus:border-white/28"
                              />
                            </label>
                            <button
                              type="button"
                              onClick={() => void handleSaveBirthday()}
                              className="btn-secondary px-4 py-2 text-sm"
                              disabled={saving}
                            >
                              Save birthday
                            </button>
                          </div>

                          <div className="space-y-3">
                            <label className="space-y-2 text-sm text-white/86">
                              <span className="block text-xs uppercase tracking-[0.14em] text-white/45">
                                Hobbies
                              </span>
                              <textarea
                                value={quickFacts.hobbies}
                                onChange={(event) =>
                                  setQuickFacts((current) => ({ ...current, hobbies: event.target.value }))
                                }
                                rows={3}
                                placeholder="cycling, baking, chess"
                                className="w-full rounded-[1rem] border border-[var(--border-subtle)] bg-transparent px-4 py-3 text-sm text-white outline-none transition focus:border-white/28"
                              />
                            </label>
                            <button
                              type="button"
                              onClick={() => void handleSaveHobbies()}
                              className="btn-secondary px-4 py-2 text-sm"
                              disabled={saving}
                            >
                              Save hobbies
                            </button>
                          </div>
                        </div>

                        <form className="grid gap-3 border-t border-[var(--border-subtle)] pt-4 md:grid-cols-[12rem_11rem_minmax(0,1fr)_auto]" onSubmit={handleSaveGenericFact}>
                          <input
                            value={quickFacts.factKey}
                            onChange={(event) =>
                              setQuickFacts((current) => ({ ...current, factKey: event.target.value }))
                            }
                            placeholder="Fact key"
                            className="h-11 rounded-[1rem] border border-[var(--border-subtle)] bg-transparent px-4 text-sm text-white outline-none transition focus:border-white/28"
                          />
                          <select
                            value={quickFacts.factType}
                            onChange={(event) =>
                              setQuickFacts((current) => ({
                                ...current,
                                factType: event.target.value as QuickFactDraft['factType'],
                              }))
                            }
                            className="h-11 rounded-[1rem] border border-[var(--border-subtle)] bg-transparent px-4 text-sm text-white outline-none transition focus:border-white/28"
                          >
                            <option value="text">text</option>
                            <option value="int">int</option>
                            <option value="bool">bool</option>
                            <option value="date">date</option>
                            <option value="json">json</option>
                          </select>
                          <input
                            value={quickFacts.factValue}
                            onChange={(event) =>
                              setQuickFacts((current) => ({ ...current, factValue: event.target.value }))
                            }
                            placeholder="Value"
                            className="h-11 rounded-[1rem] border border-[var(--border-subtle)] bg-transparent px-4 text-sm text-white outline-none transition focus:border-white/28"
                          />
                          <button className="btn-secondary px-4 py-2 text-sm disabled:opacity-60" disabled={saving}>
                            Save fact
                          </button>
                        </form>

                        <div className="grid gap-2 md:grid-cols-2">
                          {selectedBundle.facts.map((fact) => (
                            <div
                              key={fact.id}
                              className="rounded-[1rem] border border-white/8 bg-white/[0.02] px-4 py-3"
                            >
                              <p className="text-xs uppercase tracking-[0.14em] text-white/45">{fact.fact_key}</p>
                              <p className="mt-1 text-sm text-white/88">{factSummaryLabel(fact)}</p>
                            </div>
                          ))}
                        </div>
                      </section>

                      <form className="space-y-4 border-t border-[var(--border-subtle)] pt-4" onSubmit={handleSaveDocument}>
                        <div className="space-y-1">
                          <h3 className="text-sm font-semibold text-white/90">Person document</h3>
                          <p className="text-sm muted">
                            Use the long-form document for life notes, history, reminders, and nuance.
                          </p>
                        </div>
                        <input
                          value={documentTitleDraft}
                          onChange={(event) => setDocumentTitleDraft(event.target.value)}
                          placeholder="Document title"
                          className="h-11 w-full rounded-[1rem] border border-[var(--border-subtle)] bg-transparent px-4 text-sm text-white outline-none transition focus:border-white/28"
                        />
                        <textarea
                          value={documentDraft}
                          onChange={(event) => setDocumentDraft(event.target.value)}
                          rows={14}
                          placeholder="Write a richer document for this person..."
                          className="w-full rounded-[1rem] border border-[var(--border-subtle)] bg-transparent px-4 py-3 text-sm text-white outline-none transition focus:border-white/28"
                        />
                        <button className="btn-primary px-5 py-2.5 text-sm disabled:opacity-60" disabled={saving}>
                          Save document
                        </button>
                      </form>
                    </div>

                    <div className="space-y-5">
                      <section className="space-y-4">
                        <div className="space-y-1">
                          <h3 className="text-sm font-semibold text-white/90">Relationships</h3>
                          <p className="text-sm muted">
                            Store directed relationships, then let the assistant resolve relative queries safely.
                          </p>
                        </div>

                        <form className="space-y-3 rounded-[1.1rem] border border-white/8 bg-white/[0.02] p-4" onSubmit={handleCreateRelationship}>
                          <select
                            value={relationshipDraft.toPersonId}
                            onChange={(event) =>
                              setRelationshipDraft((current) => ({ ...current, toPersonId: event.target.value }))
                            }
                            className="h-11 w-full rounded-[1rem] border border-[var(--border-subtle)] bg-transparent px-4 text-sm text-white outline-none transition focus:border-white/28"
                          >
                            <option value="">Select person</option>
                            {workspacePeople
                              .filter((person) => person.id !== selectedBundle.person.id)
                              .map((person) => (
                                <option key={person.id} value={person.id}>
                                  {person.display_name}
                                </option>
                              ))}
                          </select>

                          <div className="grid gap-3 md:grid-cols-2">
                            <input
                              value={relationshipDraft.relationType}
                              onChange={(event) =>
                                setRelationshipDraft((current) => ({
                                  ...current,
                                  relationType: event.target.value,
                                }))
                              }
                              placeholder="Relation type"
                              className="h-11 rounded-[1rem] border border-[var(--border-subtle)] bg-transparent px-4 text-sm text-white outline-none transition focus:border-white/28"
                            />
                            <input
                              value={relationshipDraft.inverseRelationType}
                              onChange={(event) =>
                                setRelationshipDraft((current) => ({
                                  ...current,
                                  inverseRelationType: event.target.value,
                                }))
                              }
                              placeholder="Inverse relation"
                              className="h-11 rounded-[1rem] border border-[var(--border-subtle)] bg-transparent px-4 text-sm text-white outline-none transition focus:border-white/28"
                            />
                          </div>

                          <div className="flex flex-wrap gap-2">
                            {DEFAULT_RELATIONS.map(([forward, inverse]) => (
                              <button
                                key={`${forward}:${inverse}`}
                                type="button"
                                onClick={() =>
                                  setRelationshipDraft((current) => ({
                                    ...current,
                                    relationType: forward,
                                    inverseRelationType: inverse,
                                  }))
                                }
                                className="rounded-full border border-white/10 px-3 py-1 text-xs text-white/74 transition hover:border-white/18 hover:bg-white/[0.04]"
                              >
                                {forward}
                              </button>
                            ))}
                          </div>

                          <textarea
                            value={relationshipDraft.sourceNote}
                            onChange={(event) =>
                              setRelationshipDraft((current) => ({ ...current, sourceNote: event.target.value }))
                            }
                            rows={3}
                            placeholder="Optional provenance note"
                            className="w-full rounded-[1rem] border border-[var(--border-subtle)] bg-transparent px-4 py-3 text-sm text-white outline-none transition focus:border-white/28"
                          />

                          <button className="btn-primary px-5 py-2.5 text-sm disabled:opacity-60" disabled={saving}>
                            Save relationship
                          </button>
                        </form>

                        <div className="space-y-2">
                          {selectedBundle.relations.length > 0 ? (
                            selectedBundle.relations.map((relation) => (
                              <div
                                key={relation.id}
                                className="rounded-[1rem] border border-white/8 bg-white/[0.02] px-4 py-3"
                              >
                                <div className="flex items-start justify-between gap-3">
                                  <div className="space-y-1">
                                    <p className={`text-sm font-medium ${relationshipTone(relation.relation_type)}`}>
                                      {relation.relation_type}
                                    </p>
                                    <p className="text-sm text-white/88">{relation.target_person_name}</p>
                                    {relation.target_summary ? (
                                      <p className="text-xs muted">{relation.target_summary}</p>
                                    ) : null}
                                  </div>
                                  <button
                                    type="button"
                                    onClick={() => void handleDeleteRelationship(relation)}
                                    className="text-xs text-rose-200/90 transition hover:text-rose-100"
                                  >
                                    Delete
                                  </button>
                                </div>
                              </div>
                            ))
                          ) : (
                            <div className="rf-flat-empty">
                              <p className="text-sm muted">No visible relationships recorded yet.</p>
                            </div>
                          )}
                        </div>
                      </section>

                      <section className="space-y-3 border-t border-[var(--border-subtle)] pt-4">
                        <h3 className="text-sm font-semibold text-white/90">Access + timing</h3>
                        <div className="space-y-2 text-sm text-white/80">
                          <div className="flex items-center justify-between gap-3">
                            <span className="muted">Linked account</span>
                            <span>{isSelectedPersonLinked ? 'Yes' : 'No'}</span>
                          </div>
                          <div className="flex items-center justify-between gap-3">
                            <span className="muted">Created</span>
                            <span>{timestampLabel(selectedBundle.person.created_ts)}</span>
                          </div>
                          <div className="flex items-center justify-between gap-3">
                            <span className="muted">Updated</span>
                            <span>{timestampLabel(selectedBundle.person.updated_ts)}</span>
                          </div>
                        </div>
                      </section>
                    </div>
                  </div>
                </>
              ) : (
                <div className="rf-flat-empty min-h-[24rem]">
                  <p className="text-sm text-white/84">
                    Choose a person from the tree to view facts, documents, and relationships.
                  </p>
                  <p className="mt-2 text-sm muted">
                    The current workspace keeps read and write access bounded server-side, so private roots stay
                    private even if the same person exists elsewhere.
                  </p>
                </div>
              )}
            </section>
          </div>
        </div>
      </section>
    </div>
  );
}

type CreatePersonRequestState = {
  displayName: string;
  summary: string;
  aliases: string;
  nodeTitle: string;
  parentNodeId: string;
};

type DictionaryTreeBranchProps = {
  node: DictionaryTreeNode;
  depth: number;
  selectedPersonId: string | null;
  expandedNodes: Set<string>;
  childrenByParent: Map<string | null, DictionaryTreeNode[]>;
  peopleById: Map<string, DictionaryPerson>;
  onSelectPerson: (personId: string) => void;
  onToggleExpanded: (nodeId: string) => void;
};

function DictionaryTreeBranch({
  node,
  depth,
  selectedPersonId,
  expandedNodes,
  childrenByParent,
  peopleById,
  onSelectPerson,
  onToggleExpanded,
}: DictionaryTreeBranchProps) {
  const children = childrenByParent.get(node.id) ?? [];
  const isExpandable = children.length > 0;
  const isExpanded = expandedNodes.has(node.id);
  const person = node.person_id ? peopleById.get(node.person_id) ?? null : null;
  const isSelected = person?.id === selectedPersonId;

  return (
    <div className="space-y-1">
      <div
        className={`flex items-center gap-2 rounded-[0.95rem] px-3 py-2 transition ${
          isSelected
            ? 'border border-white/14 bg-white/[0.07]'
            : 'border border-transparent hover:border-white/10 hover:bg-white/[0.03]'
        }`}
        style={{ marginLeft: `${depth * 14}px` }}
      >
        {isExpandable ? (
          <button
            type="button"
            onClick={() => onToggleExpanded(node.id)}
            className="text-xs text-white/55 transition hover:text-white/82"
          >
            {isExpanded ? '−' : '+'}
          </button>
        ) : (
          <span className="inline-block w-3 text-center text-white/28">·</span>
        )}

        {person ? (
          <button
            type="button"
            onClick={() => onSelectPerson(person.id)}
            className="flex min-w-0 flex-1 items-center justify-between gap-3 text-left"
          >
            <span className="truncate text-sm text-white/90">{node.title}</span>
            {person.summary ? <span className="truncate text-xs muted">{person.summary}</span> : null}
          </button>
        ) : (
          <div className="flex min-w-0 flex-1 items-center justify-between gap-3">
            <span className="truncate text-sm font-medium text-white/82">{node.title}</span>
            <span className="text-[0.65rem] uppercase tracking-[0.16em] text-white/35">{node.node_kind}</span>
          </div>
        )}
      </div>

      {isExpandable && isExpanded ? (
        <div className="space-y-1">
          {children.map((child) => (
            <DictionaryTreeBranch
              key={child.id}
              node={child}
              depth={depth + 1}
              selectedPersonId={selectedPersonId}
              expandedNodes={expandedNodes}
              childrenByParent={childrenByParent}
              peopleById={peopleById}
              onSelectPerson={onSelectPerson}
              onToggleExpanded={onToggleExpanded}
            />
          ))}
        </div>
      ) : null}
    </div>
  );
}

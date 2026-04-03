# Rustyfin AI Personal Memory, Human Dictionary, and Knowledge Collections Delta Plan

Date: 2026-04-02
Status: proposed design delta and future-agent implementation brief
Scope: explicit personal memory, people and group memory, account-to-person mapping, knowledge collections, and memory-aware AI retrieval

## Purpose

Rustyfin AI is now a grounded local assistant with conversation persistence, confirmation-gated calendar writes, voice input, downloadable document generation, and live runtime telemetry.

What it does not have yet is a durable model of the user's world.

This document defines the next major delta:

- explicit personal memory for useful facts
- a structured "human dictionary" for people, groups, and relationships
- importable knowledge collections for task-specific reference material
- clear UX and storage rules so the assistant becomes more helpful without becoming vague, invasive, or slow

This document is written so a future coding agent can implement the feature set with minimal ambiguity.

## How To Use This Document

This file should be treated as the top-level authority for this feature family.

Future AI agents should use it like this:

1. Read `README.md`, `AGENTS.md`, and the current AI architecture docs first.
2. Read this file second.
3. If implementing only one phase, read only:
   - `Executive Summary`
   - `Current State`
   - the relevant `Phase` section
   - `Data Model`
   - `Assistant Tooling Model`
   - `Security And Privacy Rules`
   - `Testing And Done Criteria`
4. Do not load unrelated implementation reports unless the work touches those exact areas.
5. If this feature family grows beyond what one agent can implement comfortably, split future execution into phase-specific prompt files, but keep this file as the canonical summary.

## Executive Summary

Rustyfin should evolve from "grounded chat over current product state" into "grounded chat plus durable user-owned memory."

The recommended product model is:

- default `/ai` chat remains the general assistant
- personal memory is explicit, not ambient
- people and groups become a first-class structured dataset, not a pile of freeform notes
- large knowledge sources are stored as collections and retrieved in chunks, not pasted wholesale into prompts
- generated documents remain exports, not the primary storage layer

The most important design decisions are:

- do not make the AI silently remember everything from every chat
- do not store person or family knowledge as giant monolithic markdown files
- do not let the model mutate memory directly without confirmation-gated server tools
- do not collapse "Rustyfin account" and "real-world person" into the same concept without an explicit link model
- do not retrieve more memory or knowledge than the current turn needs

## Why This Delta Exists

The current AI can answer grounded questions, write to calendars safely, and create downloadable documents.

It still cannot reliably do things like:

- remember that the user likes dark green
- remember that Annabelle is the user's mother
- remember that Rachel hates coriander
- organize people into families, friends, and coworkers
- answer "who in my family has birthdays coming up?"
- consult a task-specific offline body of knowledge without depending on ad hoc browsing

Those are exactly the kinds of capabilities that make an assistant feel persistent and personally useful.

## Current State

### What already exists

Rustyfin AI already has:

- authenticated `/ai` chat
- per-user persisted conversations in PostgreSQL
- grounded backend-owned tools for Rustyfin domains
- confirmation-gated calendar create and delete actions
- confirmation-gated downloadable markdown and plain-text document generation
- authenticated AI artifact download route
- deterministic current date and time responses
- deterministic network responses for LAN connect questions

### What does not exist yet

Rustyfin AI does not yet have:

- durable personal memory facts
- durable people records
- group or family records
- relationship graphs between people
- account-to-person identity linking
- memory-specific CRUD routes
- knowledge collections or document ingestion
- chunked knowledge retrieval
- memory-aware assistant routing

### Important distinction

Rustyfin already persists:

- conversations
- admin audit history
- generated downloadable artifacts

Those are not a substitute for memory.

Conversation history is a transcript, not a curated fact store.
Generated artifacts are outputs, not the source of truth.
Admin audit is an operational log, not user memory.

## Adopted Product Decisions

### 1. Memory is explicit, not ambient

The assistant must not silently store arbitrary facts from normal conversation by default.

Allowed patterns:

- `Remember that my favorite color is dark green`
- `Save that Rachel hates coriander`
- `Store that Mum means Annabelle`
- `Add Annabelle to my family`

Not allowed as a default pattern:

- "the AI remembers anything the user ever mentioned"

Why:

- accidental storage is hard to trust
- stale or joking statements become false memory
- users need a clear boundary around what is being kept

### 2. Human Dictionary is structured, not document-first

People and groups should be stored as typed records with facts and relationships.

Do not use one markdown note per person as the primary store.

The primary store should be:

- person records
- group records
- membership edges
- relationship edges
- typed facts
- aliases

Optional generated summaries can exist as caches or exports, but the structured records stay authoritative.

### 3. Knowledge collections are retrieval-first, not prompt-dump-first

Large knowledge bases are possible, but they must be ingested as collections and retrieved selectively.

Do not:

- paste full manuals into the model prompt
- store giant documents as one row and inject them verbatim
- build one mega "everything I know" document

Do:

- keep the raw source
- extract text
- split it into chunks
- index the chunks
- retrieve only the relevant subset

### 4. `/ai` remains the primary chat surface

The current `/ai` page should stay the primary assistant surface.

Recommended UX extension:

- default chat mode: `General`
- optional mode: `Memory`
- optional mode: `People`
- optional mode: `Knowledge`

This can begin as a mode switch inside `/ai`.
It does not need a separate top-level app area on day one.

### 5. Memory writes must remain confirmation-gated

The assistant already has a confirmation-token model for write-capable actions.

Memory and human-dictionary writes must reuse that pattern.

Examples:

- create person
- update person fact
- create group
- add group member
- link account to person
- save relationship
- ingest knowledge collection
- delete or overwrite memory facts

### 6. Account identity and real-world identity must be separate concepts

Rustyfin account != person by default.

The system needs an explicit linking layer because:

- some useful people do not have Rustyfin accounts
- some Rustyfin accounts belong to people the current user knows in real life
- the same person can be referred to by username, display name, role title, or family role

The assistant must understand these as linkable but distinct records.

## Recommended Product Model

### Layers

The feature family should be built as three connected layers.

### Layer A: Personal Memory

This is the user's private fact memory.

Examples:

- favorite colors
- food preferences
- routine preferences
- reminders about names or aliases
- household conventions
- gift ideas

Use cases:

- `What color do I usually prefer?`
- `Remember that I do not like coriander`
- `What do you know about my preferences?`

### Layer B: Human Dictionary

This is the people and group graph.

Examples:

- people: Annabelle, Rachel, James
- groups: Family, Friends, Coworkers
- relationships: mother, brother, manager, teammate
- aliases: Mum, Mam, Annabelle

Use cases:

- `Who is in my family?`
- `Annabelle on the server is my mother`
- `Who has birthdays coming up in my family?`
- `What do you know about Rachel?`

### Layer C: Knowledge Collections

This is task-specific reference material.

Examples:

- home network documentation
- appliance manuals
- medication notes
- tax checklists
- travel notes
- house procedures
- work SOPs

Use cases:

- `What does the boiler manual say about pressure reset?`
- `Search my networking notes for the Jellyfin reverse proxy config`
- `Summarize the tax checklist collection`

## How To Keep This Efficient

This section is adopted and should guide implementation decisions.

### Do not store giant composite documents as the source of truth

Bad pattern:

- one enormous "family.md"
- one enormous "personal memory.md"
- one enormous "home server knowledge base.md"

Why it is bad:

- slow retrieval
- poor update semantics
- hard conflict handling
- too much irrelevant prompt context

### Use structured facts and chunked retrieval instead

The efficient storage model is:

- atomic structured facts for memory and people
- cached short summaries for quick UI display
- chunked full-text retrieval for large documents

### Retrieval budgets must be enforced server-side

The assistant should not receive unbounded memory context.

Recommended first-pass limits:

- max 20 memory facts returned by a memory-search tool
- max 12 visible facts in a person summary tool
- max 12 visible relationships in a person summary tool
- max 8 retrieved knowledge chunks for one answer
- max 1000 characters per retrieved chunk after normalization
- max 8000 characters total retrieved knowledge text injected into one turn

These are product safety rules, not just tuning suggestions.

### Store summaries as caches, not authority

Each person, group, and collection may have a short cached summary for UI display and quick preview.

That summary must be derived from structured facts and indexed chunks.
It must never become the only source of truth.

## Integration With Current Rustyfin Architecture

## Existing ownership that should remain central

- backend AI route orchestration:
  - `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_enabled.rs`
- grounded AI module:
  - `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant`
- conversation persistence:
  - `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_conversations.rs`
- assistant confirmation flow:
  - `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/confirmation.rs`
- generated AI artifact downloads:
  - `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_generated_artifacts.rs`
- DB repos and migrations:
  - `/Users/iwanteague/Desktop/Rustyfin/crates/db`

## New backend areas recommended

- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_memory.rs`
  - HTTP routes for memory, people, groups, and knowledge collections
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/memory.rs`
  - assistant tools for personal memory
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/people.rs`
  - assistant tools for people, groups, and relationships
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/knowledge.rs`
  - assistant tools for knowledge collections and retrieval

## New frontend areas recommended

- `/Users/iwanteague/Desktop/Rustyfin/ui/src/features/ai-memory/`
- `/Users/iwanteague/Desktop/Rustyfin/ui/src/features/ai-memory/components/`
- `/Users/iwanteague/Desktop/Rustyfin/ui/src/features/ai-memory/api.ts`
- `/Users/iwanteague/Desktop/Rustyfin/ui/src/features/ai-memory/types.ts`

Likely components:

- `AiModeSwitcher`
- `AiMemoryFactList`
- `AiPersonDirectory`
- `AiPersonDetail`
- `AiGroupDirectory`
- `AiKnowledgeCollectionList`
- `AiKnowledgeCollectionDetail`

## Reuse rules

- reuse `ai_assistant_confirmation` for write confirmation
- reuse `ai_generated_artifacts` for exports and downloadable summaries
- reuse `ai_conversation` only for chat history, not for the memory source of truth
- reuse current SSE and chat transport instead of inventing a separate assistant protocol

## Data Model

The following schema design is recommended.

Field names are intentionally concrete so a future implementation agent does not have to invent the core model.

## Core tables

### `ai_person`

Purpose:

- canonical person record

Recommended fields:

- `id TEXT PRIMARY KEY`
- `owner_user_id TEXT NOT NULL`
- `display_name TEXT NOT NULL`
- `sort_name TEXT`
- `kind TEXT NOT NULL`
  - expected values: `self`, `account_linked`, `external`
- `linked_user_id TEXT NULL`
- `is_active BOOLEAN NOT NULL DEFAULT TRUE`
- `summary_text TEXT NULL`
- `created_ts BIGINT NOT NULL`
- `updated_ts BIGINT NOT NULL`

Rules:

- `owner_user_id` is the user who owns this memory graph entry
- `linked_user_id` links to a Rustyfin account only when explicitly mapped
- other users do not automatically gain write access to this record

### `ai_person_alias`

Purpose:

- alternate names for entity resolution

Recommended fields:

- `id TEXT PRIMARY KEY`
- `person_id TEXT NOT NULL`
- `alias TEXT NOT NULL`
- `normalized_alias TEXT NOT NULL`
- `created_ts BIGINT NOT NULL`

### `ai_group`

Purpose:

- user-scoped groups such as Family, Friends, Coworkers

Recommended fields:

- `id TEXT PRIMARY KEY`
- `owner_user_id TEXT NOT NULL`
- `name TEXT NOT NULL`
- `group_type TEXT NOT NULL`
  - expected values: `family`, `friends`, `coworkers`, `custom`
- `summary_text TEXT NULL`
- `created_ts BIGINT NOT NULL`
- `updated_ts BIGINT NOT NULL`

### `ai_group_member`

Purpose:

- membership edge between groups and people

Recommended fields:

- `id TEXT PRIMARY KEY`
- `group_id TEXT NOT NULL`
- `person_id TEXT NOT NULL`
- `role_label TEXT NULL`
- `created_ts BIGINT NOT NULL`

### `ai_person_relationship`

Purpose:

- explicit person-to-person relationship edges

Recommended fields:

- `id TEXT PRIMARY KEY`
- `owner_user_id TEXT NOT NULL`
- `from_person_id TEXT NOT NULL`
- `to_person_id TEXT NOT NULL`
- `relationship_type TEXT NOT NULL`
  - expected values initially:
    - `mother`
    - `father`
    - `parent`
    - `brother`
    - `sister`
    - `sibling`
    - `partner`
    - `spouse`
    - `friend`
    - `coworker`
    - `manager`
    - `child`
    - `custom`
- `custom_label TEXT NULL`
- `created_ts BIGINT NOT NULL`
- `updated_ts BIGINT NOT NULL`

Rule:

- relationships are owner-scoped in the first implementation
- this avoids editing another user's canonical self profile without consent

### `ai_memory_fact`

Purpose:

- atomic user-owned facts

Recommended fields:

- `id TEXT PRIMARY KEY`
- `owner_user_id TEXT NOT NULL`
- `subject_type TEXT NOT NULL`
  - expected values: `user`, `person`, `group`
- `subject_id TEXT NOT NULL`
- `fact_key TEXT NOT NULL`
  - expected examples:
    - `favorite_color`
    - `food_preference`
    - `birthday`
    - `gift_idea`
    - `allergy`
    - `nickname`
    - `note`
- `fact_value TEXT NOT NULL`
- `value_json TEXT NULL`
- `source_kind TEXT NOT NULL`
  - expected values: `assistant_write`, `manual_ui`, `import`
- `visibility TEXT NOT NULL`
  - expected initial values: `private`
- `confidence TEXT NOT NULL`
  - expected initial values: `user_asserted`
- `created_ts BIGINT NOT NULL`
- `updated_ts BIGINT NOT NULL`

Rule:

- this is the primary personal-memory store
- freeform notes can exist, but typed keys should be preferred where possible

### `ai_knowledge_collection`

Purpose:

- task- or domain-specific knowledge container

Recommended fields:

- `id TEXT PRIMARY KEY`
- `owner_user_id TEXT NOT NULL`
- `name TEXT NOT NULL`
- `description TEXT NULL`
- `collection_type TEXT NOT NULL`
  - expected values: `personal`, `household`, `work`, `task`, `reference`
- `summary_text TEXT NULL`
- `created_ts BIGINT NOT NULL`
- `updated_ts BIGINT NOT NULL`

### `ai_knowledge_document`

Purpose:

- one source document inside a collection

Recommended fields:

- `id TEXT PRIMARY KEY`
- `collection_id TEXT NOT NULL`
- `owner_user_id TEXT NOT NULL`
- `title TEXT NOT NULL`
- `source_kind TEXT NOT NULL`
  - expected values: `upload`, `paste`, `public_url`, `generated`
- `source_ref TEXT NULL`
- `file_name TEXT NULL`
- `media_type TEXT NULL`
- `storage_path TEXT NULL`
- `extracted_text TEXT NULL`
- `summary_text TEXT NULL`
- `ingest_status TEXT NOT NULL`
  - expected values: `pending`, `ready`, `error`
- `created_ts BIGINT NOT NULL`
- `updated_ts BIGINT NOT NULL`

Rule:

- raw source files for larger imports should live on disk
- extracted normalized text and chunk metadata should live in PostgreSQL

### `ai_knowledge_chunk`

Purpose:

- chunked retrieval unit for one ingested document

Recommended fields:

- `id TEXT PRIMARY KEY`
- `document_id TEXT NOT NULL`
- `collection_id TEXT NOT NULL`
- `chunk_index INTEGER NOT NULL`
- `text_content TEXT NOT NULL`
- `token_estimate INTEGER NOT NULL`
- `tsv TSVECTOR NULL` or equivalent searchable representation
- `created_ts BIGINT NOT NULL`

Rule:

- v1 should prefer PostgreSQL lexical or full-text retrieval
- embeddings are optional later, not required for the first implementation

## Account And Family Semantics

This is the part most likely to become ambiguous during implementation, so it is defined explicitly here.

### Self profile

Each authenticated Rustyfin user may have one self-owned person record.

Expected flow:

- user says `I am Iwan`
- system creates or confirms a self profile
- user can store self facts against that profile

### External person

A user may create person records for real-world people who do not have Rustyfin accounts.

Examples:

- grandparents
- children
- coworkers
- neighbors

### Account-linked person

If a Rustyfin account exists, the system may link that account to a person record.

Examples:

- `Annabelle on the server is my mother`
- `James in Rustyfin is my brother`

### Critical rule for v1

Do not allow one user to overwrite another user's canonical self-owned facts simply because they linked that account in their own graph.

Instead:

- canonical self facts are owned by the account holder
- relationship assertions are owner-scoped first
- "Annabelle is my mother" is primarily stored as a relationship in the current user's graph

This keeps the feature useful without requiring collaborative identity governance in the first release.

### Family groups

Groups such as `Family` should be user-scoped first.

That means:

- the user can create a Family group
- the user can add linked or external people to it
- the group helps the assistant answer questions like `who in my family has birthdays coming up?`

Collaborative family groups can be a later phase.

## Assistant Tooling Model

The assistant should gain new tools in three categories.

## Read-only tools

Recommended initial tools:

- `memory_list_recent_facts`
- `memory_search_facts`
- `person_search`
- `person_get_summary`
- `group_list`
- `group_get_summary`
- `knowledge_list_collections`
- `knowledge_search_collection`
- `knowledge_get_document_summary`

## Confirmation-gated write tools

Recommended initial tools:

- `memory_save_fact`
- `memory_delete_fact`
- `person_create`
- `person_add_alias`
- `person_link_account`
- `group_create`
- `group_add_member`
- `relationship_save`
- `knowledge_create_collection`
- `knowledge_add_text_document`
- `knowledge_add_public_url_document`
- `knowledge_delete_document`

## Export tools

Recommended initial tools:

- `memory_export_person_summary`
- `memory_export_group_summary`
- `knowledge_export_collection_summary`

These should reuse the existing AI-generated artifact path for downloadable outputs.

## Retrieval rules

The assistant should obey this retrieval order:

1. structured person/group/memory facts
2. cached summaries
3. knowledge chunk retrieval
4. final model answer

The model should not be allowed to skip directly to knowledge-search when a structured fact answer already exists.

## Proposed Route Plan

These routes are recommended so the implementation agent does not have to invent the public surface.

### User-facing routes

- `GET /api/v1/ai/memory/facts`
  - list or search current-user memory facts
- `POST /api/v1/ai/memory/facts`
  - create a fact through normal authenticated UI, still requiring assistant confirmation when invoked from chat
- `DELETE /api/v1/ai/memory/facts/{id}`
  - delete a fact
- `GET /api/v1/ai/people`
  - list current-user people
- `POST /api/v1/ai/people`
  - create person
- `GET /api/v1/ai/people/{id}`
  - get person detail
- `PATCH /api/v1/ai/people/{id}`
  - update person
- `POST /api/v1/ai/people/{id}/aliases`
  - add alias
- `POST /api/v1/ai/people/{id}/link-account`
  - link person to Rustyfin account
- `GET /api/v1/ai/groups`
  - list current-user groups
- `POST /api/v1/ai/groups`
  - create group
- `GET /api/v1/ai/groups/{id}`
  - get group detail
- `POST /api/v1/ai/groups/{id}/members`
  - add member
- `POST /api/v1/ai/relationships`
  - create owner-scoped relationship assertion
- `GET /api/v1/ai/knowledge/collections`
  - list knowledge collections
- `POST /api/v1/ai/knowledge/collections`
  - create collection
- `GET /api/v1/ai/knowledge/collections/{id}`
  - get collection detail
- `POST /api/v1/ai/knowledge/collections/{id}/documents/text`
  - ingest pasted text or markdown
- `POST /api/v1/ai/knowledge/collections/{id}/documents/upload`
  - upload a supported file
- `POST /api/v1/ai/knowledge/collections/{id}/documents/public-url`
  - ingest explicit public URL content
- `DELETE /api/v1/ai/knowledge/documents/{id}`
  - delete ingested document

### Assistant-only behavior

The assistant should still perform most creation and lookup through server-side tools inside `/api/v1/ai/chat` and `/api/v1/ai/conversations/{id}/messages/stream`.

These CRUD routes are for:

- management UI
- correction flows
- trust and inspectability
- future non-chat surfaces

They are not a replacement for backend-owned assistant tools.

## Representative End-To-End Flows

These examples are intentionally concrete so future implementation does not drift.

### Flow 1: Favorite color memory

User prompt:

- `Remember that my favorite color is dark green`

Expected system behavior:

1. planner routes to `memory_save_fact`
2. assistant prepares confirmation card summarizing:
   - subject: self
   - key: `favorite_color`
   - value: `dark green`
3. user confirms
4. backend writes `ai_memory_fact`
5. assistant replies that the memory was saved

Follow-up prompt:

- `What is my favorite color?`

Expected behavior:

1. planner routes to `memory_search_facts`
2. backend finds the self-scoped fact
3. assistant returns `Your stored favorite color is dark green.`

### Flow 2: Account-linked family relationship

User prompt:

- `Annabelle on the server is my mother`

Expected system behavior:

1. backend resolves Rustyfin account `Annabelle`
2. backend ensures the current user has a self profile
3. backend creates or resolves an account-linked person record for Annabelle in the current user's graph
4. assistant prepares confirmation for:
   - linking the account reference
   - storing relationship `mother`
   - optionally adding Annabelle to `Family` if the user asked for that
5. user confirms
6. backend writes the relationship
7. future family and birthday questions can use that structured edge

Critical rule:

- this does not give the current user authority to edit Annabelle's canonical self facts

### Flow 3: Task knowledge collection

User prompt:

- `Create a collection called Home Network and add this markdown`

Expected system behavior:

1. assistant prepares confirmation for collection creation and document ingest
2. backend creates `ai_knowledge_collection`
3. backend stores the source document, extracts text, and chunks it
4. backend marks ingest status `ready`
5. follow-up queries can retrieve matching chunks from that collection only

Follow-up prompt:

- `Search Home Network for the local Rustyfin IP and reverse proxy port`

Expected behavior:

1. planner routes to `knowledge_search_collection`
2. backend retrieves a bounded set of relevant chunks
3. assistant answers with grounded excerpts and clear source attribution

## UX Shape

## Default recommendation

Do not make this a completely separate assistant app.

Recommended product shape:

- keep `/ai` as the main route
- add a visible mode switch
- add optional management panels or drawers for People and Knowledge

### Modes

Recommended initial modes:

- `General`
  - normal assistant chat
  - may consult memory if the user enables memory for the conversation
- `Memory`
  - optimized for saving and recalling personal facts
- `People`
  - optimized for human-dictionary questions
- `Knowledge`
  - optimized for collection-specific retrieval

### Write behavior in chat

When the user asks to save memory, create a person, create a group, or ingest knowledge:

- the assistant should prepare a confirmation card
- the assistant should not claim success until the server write completes
- the final answer should cite what was stored and where

### Management surfaces

Recommended later additions:

- person directory
- person detail view
- group directory
- collection directory
- collection detail view

These are valuable for trust and correction, but they do not need to block the first AI-driven release.

## Knowledge Collection Strategy

The user explicitly asked whether Rustyfin could download or store large knowledge bases for specific tasks.

Yes, but only with a bounded ingestion model.

### Allowed source types for the first useful version

- pasted text
- uploaded `.txt`
- uploaded `.md`
- uploaded `.pdf`
- fetched public URL content with explicit confirmation
- generated summaries exported back out as downloadable artifacts

### Deferred source types

- giant recursive website crawls
- arbitrary private-site crawling
- automatic background sync from arbitrary internet sources
- unrestricted ingestion without collection boundaries

### Why collections matter

Collections give the assistant a bounded search target.

Examples:

- `Home Network`
- `Family Notes`
- `Household Manuals`
- `Tax Checklist`
- `Work SOPs`

Then the assistant can answer:

- `Search my Home Network collection for the Rustyfin reverse proxy port`

instead of searching every scrap of remembered data.

## Security And Privacy Rules

These are mandatory.

### Memory rules

- no passive memory from all chat by default
- all writes must be confirmation-gated
- all reads and writes must stay user-scoped unless explicitly designed otherwise
- memory facts should be editable and deletable

### Person and relationship rules

- do not let one user modify another account holder's canonical self profile without an explicit shared model
- owner-scoped relationship assertions are allowed in v1
- canonical self facts and third-party relationship assertions must stay distinguishable

### Knowledge rules

- do not ingest private or authenticated web content via generic fetch
- keep current public-web safety boundaries intact
- large uploaded files should be normalized and chunked server-side
- retrieval should be bounded and logged

### Model rules

- the model must never be the authority for stored facts
- the backend remains the authority for persistence, retrieval, access, and confirmation

## Recommended Phased Implementation

## Phase 0: Foundation

Goal:

- add the core schema, repos, and routing surfaces without turning on memory-aware assistant behavior yet

Implement:

- DB migrations for people, groups, memory facts, knowledge collections, documents, and chunks
- DB repos under `crates/db/src/repo/`
- server routes under `crates/server/src/ai_memory.rs`
- confirmation-gated write plumbing in the assistant

Done when:

- CRUD works for the new records under auth
- unit and integration coverage exists for auth and ownership boundaries

## Phase 1: Explicit Personal Memory

Goal:

- let users save and recall atomic personal facts

Implement:

- `memory_save_fact`
- `memory_search_facts`
- `memory_delete_fact`
- memory mode in `/ai`
- confirmation cards for save and delete

Example supported prompts:

- `Remember that my favorite color is dark green`
- `What is my favorite color?`
- `Delete the memory that I like coriander`

Done when:

- saves are explicit
- recall is deterministic when a fact exists
- deletes are confirmation-gated

## Phase 2: Human Dictionary

Goal:

- let users build durable people and group memory

Implement:

- person create/search/summary tools
- group create/add-member/summary tools
- relationship save tool
- people mode in `/ai`

Example supported prompts:

- `Create a person called Rachel`
- `Add Rachel to my Family group`
- `Annabelle on the server is my mother`
- `Who is in my family?`

Done when:

- the assistant can answer group and relationship questions from structured data
- people are not stored as giant freeform notes

## Phase 3: Account-To-Person Linking

Goal:

- let the assistant understand that a Rustyfin user account may represent a real-world person

Implement:

- `person_link_account`
- lookup by username and display name
- self-profile creation and claim flow
- relationship rules for linked accounts

Example supported prompts:

- `Annabelle on the server is my mother`
- `Link my account to my self profile`
- `Is James on the server already in my family group?`

Done when:

- assistant can resolve account-linked people safely
- cross-user canonical data is still protected

## Phase 4: Knowledge Collections

Goal:

- let users ingest task-specific reference material and query it efficiently

Implement:

- collection CRUD
- document ingest
- text extraction
- chunking
- lexical retrieval
- knowledge mode in `/ai`

Example supported prompts:

- `Create a collection called Home Network`
- `Add this markdown to Home Network`
- `Search Home Network for Jellyfin reverse proxy`

Done when:

- retrieval is chunked and bounded
- raw large documents are not prompt-dumped

## Phase 5: Export And Trust UX

Goal:

- let users inspect, correct, and export stored knowledge

Implement:

- exports to AI artifact downloads
- person summary export
- group summary export
- collection summary export
- lightweight people and collection management UI

Done when:

- the user can see what is stored
- the user can download summaries
- the user can correct mistakes without chat-only workflows

## Testing And Done Criteria

Every implementation phase should include:

- `cargo fmt --all`
- `cargo check`
- `cargo check -p rustfin-server --features ai`
- `cargo test -p rustfin-server --features ai --lib`
- relevant `crates/db` tests
- `npm --prefix ui run build`

Additional required tests:

- auth ownership boundaries
- confirmation-token correctness
- person/account linking rules
- relationship scoping
- collection retrieval bounds
- no giant memory payload injection into prompt assembly

## Rules For The Future Implementing Agent

The future agent implementing this plan should follow these rules exactly:

- do not implement ambient always-on memory first
- do not store people as raw markdown documents first
- do not skip confirmation for memory writes
- do not treat conversation history as canonical memory
- do not let the model decide which facts become durable without backend validation
- do not merge Rustyfin account identity and real-world identity without an explicit link table
- do not implement giant unchunked knowledge imports
- do not inject entire collections into prompts
- do not expose arbitrary filesystem write or browse capability to satisfy knowledge features

## Open Questions And Deferred Work

These questions are real, but they should not block the first useful implementation:

- should collaborative shared family groups exist, or should groups stay owner-scoped longer?
- should account-linked person profiles require opt-in from the linked user before becoming canonical?
- should knowledge retrieval later add embeddings, or is PostgreSQL lexical retrieval enough for the first shipped version?
- should memory facts support visibility beyond `private` in the first release?
- should people and collections eventually become separate pages outside `/ai`, or remain tabs within `/ai`?

## Recommended Next Document Split

This file is intentionally comprehensive.

If implementation begins, the next split should be:

- one phase-specific execution prompt per phase
- one schema contract doc if the tables become materially more complex
- one UX wireframe doc only if `/ai` navigation or management views become materially larger

Until then, this file should remain the single authoritative delta document for:

- personal memory
- human dictionary
- account/person linking
- knowledge collections

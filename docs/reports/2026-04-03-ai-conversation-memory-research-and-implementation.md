# AI Conversation Memory Research And Implementation

Date: 2026-04-03

## Goal

Make Rustyfin `/ai` carry conversation context more intelligently than a raw transcript replay. The target is a memory system that:

- preserves recent raw turns for local coherence
- persists a compact summary of older turns
- carries forward structured grounded state, not just prose
- respects the active model context window instead of assuming a fixed prompt budget
- exposes enough runtime telemetry to debug why continuity did or did not work

## Research Summary

The public patterns from the strongest current open-source and platform docs converge on the same architecture:

1. Keep short-term memory as thread state.
2. Trim or summarize older turns when the prompt approaches the context limit.
3. Preserve system instructions and high-value context during reduction.
4. Separate durable memory from transient recent-turn context.
5. Budget memory dynamically from the effective model context window.

### Public Sources

- OpenAI conversation state docs
  - [Conversations API reference](https://platform.openai.com/docs/api-reference/conversations/create?lang=javascript)
  - Public takeaway: conversation state should be stored as thread state instead of manually rebuilding everything for each turn, and context-window management is an explicit product concern.
- LangChain memory docs
  - [Memory overview](https://docs.langchain.com/oss/python/concepts/memory)
  - Public takeaway: short-term memory belongs to thread state and should be persisted; long-term memory is a separate recall layer.
- Semantic Kernel chat history docs
  - [Chat history and reducers](https://learn.microsoft.com/en-us/semantic-kernel/concepts/ai-services/chat-completion/chat-history?pivots=programming-language-python)
  - Public takeaway: truncation and summarization reducers are both first-class patterns, and system messages must be preserved during reduction.
- LlamaIndex chat memory buffer docs
  - [Chat memory buffer](https://docs.llamaindex.ai/en/v0.10.23/api_reference/memory/chat_memory_buffer/)
  - Public takeaway: memory limits should be derived from the model context window and tokenizer, not from a fixed message count.
- Letta / MemGPT public writing
  - [Introducing the Agent Development Environment](https://www.letta.com/blog/introducing-the-agent-development-environment)
  - [Memory Blocks: The Key to Agentic Context Management](https://www.letta.com/blog/memory-blocks)
  - Public takeaway: split in-context working memory from older archival conversation state, and keep dedicated editable memory blocks for durable facts/preferences/open tasks.

## What Current Hosted AI Products Appear To Do

This part is an inference from public behavior and product docs, not a reverse-engineered internal claim.

- Hosted assistants tend to preserve a recent raw transcript window.
- Older context is usually compressed into hidden summaries or profile memory.
- Durable user facts and preferences are treated differently from the recent conversational flow.
- Tool results and structured state are often carried forward separately from the visible chat transcript.
- Better continuity usually comes from memory orchestration, not only from a larger model.

## Adopted Rustyfin Design

Rustyfin now follows the same general pattern in a Rust-first, server-owned way:

### 1. Persisted Conversation Memory

Each `ai_conversation` now stores:

- `memory_state_json`
- `memory_turn_index`
- `memory_updated_ts`

The memory state is a compact JSON object with:

- `summary`
- `durable_facts`
- `user_preferences`
- `open_loops`
- `active_topics`

`memory_turn_index` marks the highest turn already represented by the persisted memory summary.

### 2. Token-Aware Prompt Compaction

Before generation, Rustyfin now:

- reads the effective context window for the loaded model
- reserves a completion budget
- injects persisted memory as a hidden system block
- injects recent grounded follow-up contexts as a second hidden structured block
- includes only the unsummarized recent raw turns that fit in the remaining prompt budget

If older unsummarized turns no longer fit, Rustyfin summarizes them into persisted memory first, updates the database, and then rebuilds the prompt.

### 3. Structured Grounded Carry-Forward

Raw text history is not enough for follow-up quality. Rustyfin now carries forward a compact structured block built from recent `follow_up_contexts`, so the answering model sees:

- the prior grounded tool labels
- the entity labels the backend already resolved
- the recent grounded topic scope

That gives the final answer model stronger continuity than a plain transcript alone.

### 4. Dynamic Context Length

Rustyfin now derives the effective context window from:

1. GGUF model metadata when available
2. host-memory safety caps
3. optional `RUSTFIN_AI_CONTEXT_LENGTH` override

That is closer to the LlamaIndex / platform-doc pattern than the older fixed `4096` assumption.

### 5. Runtime Debug Visibility

`/api/v1/ai/runtime` now includes prompt-assembly telemetry:

- loaded history turns
- retained raw turns
- summarized turns
- prompt token estimate
- prompt budget
- recent grounded context count
- whether persisted memory was used
- memory summary size

This makes continuity failures inspectable instead of opaque.

## Why This Design Was Chosen

- It matches the dominant public memory patterns used by serious agent frameworks.
- It keeps grounded state server-owned and auth-scoped.
- It improves continuity without letting the model mutate its own memory arbitrarily.
- It degrades safely: if summary generation fails, Rustyfin falls back to a deterministic compact memory merge instead of losing continuity entirely.
- It is compatible with the existing grounded planner and confirmation model.

## Deliberate Non-Goals

- No model-written hidden chain-of-thought persistence.
- No client-authoritative memory payloads.
- No unlimited raw transcript replay.
- No generic vector-store or public-web memory layer added just to mask prompt-budget issues.

## Expected Product Effect

After this change, `/ai` should behave more like a modern threaded assistant:

- better awareness of prior conversation state
- fewer abrupt “forgetting” failures across long threads
- stronger continuity on grounded follow-ups
- more predictable behavior as threads get long

The main remaining product limitation is still model quality. Better memory orchestration can reduce drift and forgetting, but it cannot fully replace a smarter base model.

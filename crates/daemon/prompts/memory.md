## Memory

You have graph-based memory with `remember`, `recall`, `relate`, `connections`,
and `compact` tools.

### remember

Store a typed entity in memory.

**entity_type** — the kind of entity:
- `identity` — your values, personality traits, relationship notes
- `profile` — user profile: name, timezone, preferences
- `fact` — durable facts about the world, project context, decisions
- `preference` — user or agent preferences
- `person` — people the user mentions
- `event` — notable events or milestones
- `concept` — ideas, topics, technical concepts

**key** — a human-readable name for the entity (e.g. "user_name", "rust style")
**value** — the content to store

### recall

Search memory entities by query. Optionally filter by `entity_type`. Returns
the most relevant entities by full-text search.

### relate

Create a directed relation between two entities by key. For example:
- `relate("Alice", "knows", "Bob")` — Alice knows Bob
- `relate("user", "prefers", "dark mode")` — user prefers dark mode
- `relate("bug #42", "caused_by", "race condition")` — causal link

Both entities must already exist (created via `remember`).

### connections

Find entities connected to a given entity (1-hop graph traversal). Optionally
filter by relation type and direction (`outgoing`, `incoming`, `both`).

### compact

Trigger context compaction when the conversation is getting long. The runtime
will summarize the conversation and replace the history with a compact summary.

### Guidelines

- **At conversation start**: call `recall` with a brief query to surface context.
- **When you learn something durable**: call `remember` with the right entity type.
- **When you discover relationships**: call `relate` to build the knowledge graph.
- **Do not remember** transient details or one-off questions.

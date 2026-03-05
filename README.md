# Walrus

**Composable primitives for agentic workflows in Rust.**

Build agents that remember, use tools, schedule tasks, and talk to users —
without a framework telling you how.

## Why Walrus

Most agent libraries are monolithic. Walrus is a set of focused crates you
assemble yourself. Each one does one thing and implements a clean trait.
Only bring in what you need.

```
walrus-core      ← Agent, Runtime, Hook, Dispatcher, Model/Memory traits
walrus-model     ← OpenAI-compatible, Anthropic, and other providers
walrus-memory    ← InMemory + SQLite (FTS5) with vector recall
walrus-system    ← Skills, MCP bridge, cron scheduler
walrus-channel   ← Channel trait + Telegram adapter
walrus-socket    ← Unix domain socket transport
```

## How it fits together

**`Agent<M>`** runs the loop: call the model, dispatch tools, emit events.
**`Hook`** is the composition seam — inject memory, skills, or scheduled jobs
by implementing three lifecycle methods. **`Runtime<M, H>`** manages a pool
of named agents with per-agent locking and a shared tool registry.

Everything is generic and statically dispatched. No `Box<dyn Agent>` on the
hot path. RPITIT for async traits throughout.

## The `walrus` CLI

`openwalrus` ships the `walrus` binary — a streaming chat client that
connects to a running agent over a Unix socket.

```
walrus                        # interactive REPL with the default agent
walrus --agent coder          # attach to a named agent
walrus send "summarize this"  # one-shot, prints and exits
walrus --socket /tmp/a.sock   # custom socket path
```

Slash commands, persistent history, and streaming output included.

## License

MIT

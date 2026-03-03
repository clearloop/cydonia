# walrus-daemon

The Walrus daemon (`walrusd`).

Runs as a background service, exposing agents over a Unix domain socket.
Manages agent configuration, skills, MCP servers, memory tools, channels,
and cron-scheduled tasks.

## Features

- `local` (default) — Local model inference via mistral.rs
- `cuda` — NVIDIA CUDA GPU acceleration
- `metal` — Apple Metal GPU acceleration

## License

GPL-3.0

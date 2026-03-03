# walrus-model

LLM provider implementations for Walrus.

Supports DeepSeek, OpenAI-compatible, Claude, and local inference (via
mistral.rs). Includes `ProviderManager` for multi-provider routing and
`ProviderConfig` for configuration.

## Features

- `local` (default) — Local model inference via mistral.rs
- `cuda` — NVIDIA CUDA GPU acceleration
- `metal` — Apple Metal GPU acceleration

## License

GPL-3.0

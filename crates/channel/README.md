# walrus-channel

Platform-agnostic messaging channel abstraction for OpenWalrus.

Provides the `Channel` trait, `ChannelHandle` for bidirectional messaging,
`ChannelMessage` with platform metadata and attachment support, and a
three-tier `ChannelRouter` that routes incoming messages to named agents.
Includes a Telegram adapter using direct `reqwest` long-polling and
`spawn_channels` for lifecycle management of all configured platform connections.

## License

GPL-3.0

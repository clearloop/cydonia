# walrus-socket

Unix domain socket transport for Walrus.

Provides `WalrusClient`, `Connection`, and `accept_loop` with length-prefixed
framing (4-byte BE u32 + JSON payload) for client-server communication.

## License

GPL-3.0

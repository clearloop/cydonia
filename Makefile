# Walrus Makefile
#
# Cross-platform builds for walrus (CLI) and walrusd (daemon).
#
# - macOS Apple Silicon: Metal acceleration
# - linux x86_64: CUDA acceleration
#
# Usage:
# make bundle
# make macos-arm64
# make macos-amd64
# make linux-arm64
# make linux-amd64
VERSION = 0.0.1
CARGO = cargo b --profile prod

# build all targets
bundle: macos-arm64 macos-amd64 linux-amd64 linux-arm64 tar-all

# make tarballs for all binaries
tar-all: tar-walrus tar-walrusd

# make tarballs for walrus
tar-walrus:
	mkdir -p target/bundle
	tar -czf target/bundle/walrus-$(VERSION)-macos-arm64.tar.gz -C target/aarch64-apple-darwin/prod walrus
	tar -czf target/bundle/walrus-$(VERSION)-macos-amd64.tar.gz -C target/x86_64-apple-darwin/prod walrus
	tar -czf target/bundle/walrus-$(VERSION)-linux-amd64.tar.gz -C target/x86_64-unknown-linux-gnu/prod walrus
	tar -czf target/bundle/walrus-$(VERSION)-linux-arm64.tar.gz -C target/aarch64-unknown-linux-gnu/prod walrus

# make tarballs for walrusd
tar-walrusd:
	mkdir -p target/bundle
	tar -czf target/bundle/walrusd-$(VERSION)-macos-arm64.tar.gz -C target/aarch64-apple-darwin/prod walrusd
	tar -czf target/bundle/walrusd-$(VERSION)-macos-amd64.tar.gz -C target/x86_64-apple-darwin/prod walrusd
	tar -czf target/bundle/walrusd-$(VERSION)-linux-amd64.tar.gz -C target/x86_64-unknown-linux-gnu/prod walrusd
	tar -czf target/bundle/walrusd-$(VERSION)-linux-arm64.tar.gz -C target/aarch64-unknown-linux-gnu/prod walrusd

# build macos-arm64 (walrusd with Metal)
macos-arm64:
	$(CARGO) --target aarch64-apple-darwin -p walrus-cli
	$(CARGO) --target aarch64-apple-darwin -p walrus-daemon --features metal

# build macos-amd64
macos-amd64:
	$(CARGO) --target x86_64-apple-darwin

# build linux-arm64
linux-arm64:
	$(CARGO) --target aarch64-unknown-linux-gnu

# build linux-amd64 (walrusd with CUDA)
linux-amd64:
	$(CARGO) --target x86_64-unknown-linux-gnu -p walrus-cli
	$(CARGO) --target x86_64-unknown-linux-gnu -p walrus-daemon

# Walrus Makefile
#
# Cross-platform builds for walrus (CLI with daemon feature).
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
VERSION = 0.0.2
CARGO = cargo b --profile prod

# build all targets
bundle: macos-arm64 macos-amd64 linux-amd64 linux-arm64 tar-all

# make tarballs for all binaries
tar-all: tar-walrus

# make tarballs for walrus
tar-walrus:
	mkdir -p target/bundle
	tar -czf target/bundle/walrus-$(VERSION)-macos-arm64.tar.gz -C target/aarch64-apple-darwin/prod openwalrus
	tar -czf target/bundle/walrus-$(VERSION)-macos-amd64.tar.gz -C target/x86_64-apple-darwin/prod openwalrus
	tar -czf target/bundle/walrus-$(VERSION)-linux-amd64.tar.gz -C target/x86_64-unknown-linux-gnu/prod openwalrus
	tar -czf target/bundle/walrus-$(VERSION)-linux-arm64.tar.gz -C target/aarch64-unknown-linux-gnu/prod openwalrus

# build macos-arm64 (Metal acceleration)
macos-arm64:
	$(CARGO) --target aarch64-apple-darwin -p openwalrus

# build macos-amd64
macos-amd64:
	$(CARGO) --target x86_64-apple-darwin -p openwalrus

# build linux-arm64
linux-arm64:
	$(CARGO) --target aarch64-unknown-linux-gnu -p openwalrus

# build linux-amd64 (CUDA acceleration)
linux-amd64:
	$(CARGO) --target x86_64-unknown-linux-gnu -p openwalrus

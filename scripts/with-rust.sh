#!/usr/bin/env bash
#
# with-rust.sh - run a command with the project-pinned Rust toolchain on PATH.
#
# Resolves the toolchain in this priority:
#   1. ASEYE_RUST_BIN env (explicit override)
#   2. asdf-rust install at ~/.asdf/installs/rust/<version>/toolchains/<triple>/bin
#   3. rustup default ($HOME/.cargo/bin)
#
# Reads version from the project .tool-versions (`rust <version>`).
# Falls back to whatever cargo is on PATH if no asdf install is found.
set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TOOL_VERSIONS="$PROJECT_ROOT/.tool-versions"

# Read pinned version
if [[ -f "$TOOL_VERSIONS" ]]; then
  RUST_VERSION="$(awk '/^rust / { print $2; exit }' "$TOOL_VERSIONS")"
else
  RUST_VERSION=""
fi

# Triple for the host
case "$(uname -m)-$(uname -s)" in
  arm64-Darwin)   TRIPLE="aarch64-apple-darwin" ;;
  x86_64-Darwin)  TRIPLE="x86_64-apple-darwin" ;;
  x86_64-Linux)   TRIPLE="x86_64-unknown-linux-gnu" ;;
  aarch64-Linux)  TRIPLE="aarch64-unknown-linux-gnu" ;;
  *-MINGW*|*-MSYS*|*-CYGWIN*) TRIPLE="x86_64-pc-windows-msvc" ;;
  *) TRIPLE="" ;;
esac

if [[ -n "${ASEYE_RUST_BIN:-}" ]]; then
  TOOLCHAIN_BIN="$ASEYE_RUST_BIN"
elif [[ -n "$RUST_VERSION" && -d "$HOME/.asdf/installs/rust/$RUST_VERSION/toolchains/$RUST_VERSION-$TRIPLE/bin" ]]; then
  TOOLCHAIN_BIN="$HOME/.asdf/installs/rust/$RUST_VERSION/toolchains/$RUST_VERSION-$TRIPLE/bin"
  export CARGO_HOME="$HOME/.asdf/installs/rust/$RUST_VERSION"
  export RUSTUP_HOME="$HOME/.asdf/installs/rust/$RUST_VERSION"
else
  TOOLCHAIN_BIN=""
fi

if [[ -n "$TOOLCHAIN_BIN" ]]; then
  export PATH="$TOOLCHAIN_BIN:$PATH"
  # An inherited RUSTUP_TOOLCHAIN (e.g. from a user's shell rc pinning a
  # different version for another project) overrides the binary on PATH
  # and silently routes cargo to whatever rustup default is installed.
  # Unsetting it ensures the project's pinned toolchain wins.
  unset RUSTUP_TOOLCHAIN
fi

exec "$@"

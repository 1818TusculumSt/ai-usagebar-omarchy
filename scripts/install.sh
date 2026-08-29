#!/usr/bin/env bash
# Full local-testing deploy from THIS checkout: build + copy the CLI
# binaries, then reset + reinstall the Omarchy Quattro plugin.
#
#   make build                     cargo release build, then the binaries
#                                  are COPIED into a writable PATH dir
#                                  (BIN_DIR override) — a real copy, never
#                                  a link into target/, which `cargo clean`
#                                  would silently uninstall
#   omarchy plugin remove         drop the current plugin install (reset)
#   omarchy plugin add <repo>     git-clone this repo into the plugins dir
#   overlay working tree          a git clone only carries COMMITTED
#                                 state; uncommitted local edits deploy
#                                 by copying plugin files over the clone
#                                 (manifest.json + omarchy/)
#
# The plugin is only the display frontend; it executes the
# `ai-usagebar-omarchy` binary from the session PATH — hence step 0.
#
# The script restarts the shell at the end: the remove+add cycle deletes and
# re-creates the plugin directory, which orphans the shell's file watchers —
# in-memory QML keeps running the OLD code even though every file on disk is
# new. `omarchy restart shell` is the reliable reload (learned the hard way:
# a "successful" deploy that the bar never showed).
#
# Env: BIN_DIR=…      where the binaries are copied (default: first
#                     writable dir on PATH — on this machine ~/.local/bin
#                     and ~/.cargo/bin are read-only mounts, so that ends
#                     up being mise's shims dir)
#      SKIP_BINARY=1  skip the build+install (plugin-only deploy)
#      SKIP_RESTART=1 skip the shell restart
set -euo pipefail

REPO=$(cd "$(dirname "$0")/.." && pwd)
PLUGIN_ID=$(jq -r '.id' "$REPO/manifest.json")
PLUGINS_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/omarchy/plugins"
TARGET="$PLUGINS_DIR/$PLUGIN_ID"

if ! command -v omarchy >/dev/null 2>&1; then
  echo "omarchy CLI not found — this script is for an Omarchy 4 machine." >&2
  exit 1
fi

# Where the binaries go: the first writable candidate. On this machine the
# usual suspects (~/.local/bin, ~/.cargo/bin) sit on read-only btrfs
# subvolume mounts, which leaves mise's SHIMS dir — writable, part of the
# session PATH (the running shell resolves through it), and NOT tool-managed
# like mise/installs/<tool>/bin, which a tool upgrade wipes.
MISE_SHIMS="${XDG_DATA_HOME:-$HOME/.local/share}/mise/shims"
writable_bin_dir() {
  local dir
  for dir in "$HOME/.local/bin" "$HOME/.cargo/bin" "$MISE_SHIMS"; do
    [[ -d $dir && -w $dir ]] && { printf '%s\n' "$dir"; return; }
  done
  echo "no writable binary dir found (tried ~/.local/bin, ~/.cargo/bin, mise shims)" >&2
  return 1
}

# 0. Build + copy the binaries the plugin executes. A symlink hack once
#    pointed mise shims straight at target/release — a `cargo clean` (or a
#    stale build) then silently changed or removed the live binary. Remove
#    any such shim everywhere, then drop a real COPY into $BIN_DIR.
if [[ ${SKIP_BINARY:-0} != 1 ]]; then
  BIN_DIR="${BIN_DIR:-$(writable_bin_dir)}"
  for dir in "$MISE_SHIMS" "$BIN_DIR"; do
    for shim in ai-usagebar-omarchy ai-usagebar-omarchy-tui; do
      if [[ -L "$dir/$shim" ]] &&
         [[ $(readlink "$dir/$shim") == "$REPO"/target/* ]]; then
        rm -- "$dir/$shim"
        echo "removed stale target/ link: $dir/$shim"
      fi
    done
  done
  make -C "$REPO" build
  install -Dm755 -- "$REPO/target/release/ai-usagebar-omarchy"     "$BIN_DIR/ai-usagebar-omarchy"
  install -Dm755 -- "$REPO/target/release/ai-usagebar-omarchy-tui" "$BIN_DIR/ai-usagebar-omarchy-tui"
  echo "binaries copied to $BIN_DIR"
fi

# 1. Reset: drop the current install (also fine when none exists yet).
omarchy plugin remove "$PLUGIN_ID" --yes >/dev/null 2>&1 || true

# 2. Install from this checkout. `plugin add` git-clones its argument, so a
#    local path works and the installed copy's origin points here — pulling
#    this repo's committed HEAD.
omarchy plugin add "$REPO" --enable --yes

# 3. Overlay the working tree so uncommitted edits deploy too. Walk with
#    RELATIVE paths (cd into the repo first, same trick as the Makefile's
#    install-plasmoid): an absolute {} would nest the whole source path
#    under $TARGET instead of mirroring omarchy/.
install -Dm644 -- "$REPO/manifest.json" "$TARGET/manifest.json"
(cd "$REPO" && find omarchy -type f -exec install -Dm644 -- {} "$TARGET/{}" \;)

# 4. Prove what landed: the binary the panel will execute, the manifest +
#    entry points, and the JS contract tests against the INSTALLED copy.
command -v ai-usagebar-omarchy
ai-usagebar-omarchy --version
omarchy plugin validate "$TARGET"
node "$TARGET/omarchy/model.test.mjs"

# 5. Reload the running shell so it actually picks the new files up.
if [[ ${SKIP_RESTART:-0} != 1 ]]; then
  omarchy restart shell
fi

echo "deployed: binary → ${BIN_DIR:-<skipped>}, plugin → $TARGET"

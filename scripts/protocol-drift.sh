#!/usr/bin/env bash
# Compares the hanabi-live revision that `hanabi_protocol` is pinned to with upstream's main, for server/protocol only.
# Writes drift.md (the report) and prints `changed=true|false` and `upstream=<sha>` for GitHub Actions to read.
# Usage: scripts/protocol-drift.sh [--no-build]
set -euo pipefail

repo_url=https://github.com/Hanabi-Live/hanabi-live
paths=(server/protocol/src server/protocol/fixtures)
root=$(cd "$(dirname "$0")/.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

pinned=$(sed -n 's/^hanabi_protocol = .*rev = "\([0-9a-f]\{40\}\)".*/\1/p' "$root/Cargo.toml")
if [[ -z $pinned ]]; then
  echo "hanabi_protocol is not pinned by a git rev in Cargo.toml" >&2
  exit 1
fi

git init -q "$work/upstream"
cd "$work/upstream"
git remote add origin "$repo_url"
git fetch -q --no-tags --filter=blob:none origin main "$pinned"
upstream=$(git rev-parse origin/main)

if git diff --quiet "$pinned" "$upstream" -- "${paths[@]}"; then
  echo "changed=false"
  echo "upstream=$upstream"
  exit 0
fi

commits=$(git log --format='- `%h` %s' "$pinned..$upstream" -- "${paths[@]}")
stat=$(git diff --stat "$pinned" "$upstream" -- "${paths[@]}")

result="not tried"
if [[ ${1:-} != --no-build ]]; then
  cp -r "$root" "$work/bot"
  cd "$work/bot"
  rm -rf target
  sed -i "s/$pinned/$upstream/" Cargo.toml
  if ! cargo update -q -p hanabi-protocol 2> "$work/update.log"; then
    result="could not update the dependency: $(tail -3 "$work/update.log")"
  elif ! cargo check --all-targets > "$work/check.log" 2>&1; then
    result="does **not** compile: $(grep -m3 -A4 '^error' "$work/check.log" | head -20)"
  elif ! cargo test --lib --test client > "$work/test.log" 2>&1; then
    result="compiles, but tests fail: $(grep -E '^test .* FAILED' "$work/test.log" | head -5)"
  else
    result="compiles and the wire and client tests pass (it may still send or expect something the tests do not cover)"
  fi
fi

cat > "$root/drift.md" <<EOF
Upstream changed \`server/protocol\` since the revision that \`hanabi_protocol\` is pinned to in \`Cargo.toml\`.

- pinned: \`$pinned\`
- upstream main: \`$upstream\`

Commits that touch the protocol:

$commits

\`\`\`
$stat
\`\`\`

The bot built with upstream main: $result

To follow: change \`rev\` of \`hanabi_protocol\` in \`Cargo.toml\`, fix what no longer compiles in \`src/wire.rs\`, and release.

<!-- upstream: $upstream -->
EOF

echo "changed=true"
echo "upstream=$upstream"

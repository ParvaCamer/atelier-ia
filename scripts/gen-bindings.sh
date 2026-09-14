#!/usr/bin/env bash
# Régénère les types TypeScript depuis les types Rust (ts-rs), puis le
# baril d'exports. Les types du frontend ne sont jamais écrits à la main :
# une divergence entre moteur et UI devient une erreur de compilation.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo test -p atelier-domain --quiet >/dev/null
out=src/ipc/generated
{
  echo "// Généré par scripts/gen-bindings.sh — ne pas modifier à la main."
  for f in "$out"/*.ts; do
    n=$(basename "$f" .ts)
    [ "$n" = "index" ] && continue
    echo "export type { $n } from \"./$n\";"
  done
} > "$out/index.ts"
echo "$(ls "$out"/*.ts | wc -l | tr -d ' ') types générés"

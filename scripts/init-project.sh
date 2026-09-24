#!/usr/bin/env bash
set -euo pipefail

repo_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
project_dir="${1:-$repo_dir}"
cd "$repo_dir"
project_dir="$(cd "$project_dir" && pwd)"

if [[ -n "${OMH_HDC:-}" ]]; then
  device_tool="$OMH_HDC"
else
  device_tool="$(command -v hdc || true)"
fi
if [[ -z "$device_tool" || ! -x "$device_tool" ]]; then
  printf 'Set OMH_HDC to the SDK hdc executable (or add it to PATH).\n' >&2
  exit 2
fi

if [[ "${OMH_SKIP_STOP:-0}" != 1 ]] && command -v omh >/dev/null 2>&1; then
  omh __stop
fi

cargo build --locked --release
cargo install --path "$repo_dir" --locked
omh --version
for attempt in {1..20}; do
  device_status="$(omh --hdc "$device_tool" devices --json)"
  if [[ "$device_status" != *'"maintenance":null'* || "$device_status" == *'"state":"blocked"'* ]]; then
    printf '%s\nDevice pool is not ready; inspect omh status before testing.\n' "$device_status" >&2
    exit 1
  fi
  if [[ "$device_status" != *'"online":false'* && "$device_status" != *'"state":"inspecting"'* && "$device_status" != *'"info":null,"online":true'* ]]; then
    break
  fi
  sleep 0.5
done
if [[ "$device_status" == *'"online":false'* || "$device_status" == *'"state":"inspecting"'* || "$device_status" == *'"info":null,"online":true'* ]]; then
  printf '%s\nDevice discovery did not finish within 10 seconds.\n' "$device_status" >&2
  exit 1
fi
printf '%s\n' "$device_status"
omh setup codex --project "$project_dir"
printf 'Project setup complete: %s\n' "$project_dir"

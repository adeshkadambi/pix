#!/usr/bin/env bash
# Compliance guard (SPEC §2). Fails if the source uses banned Win32 APIs or if the League client
# HTTP module sends anything other than GET.
#
#   scripts/compliance.sh               check the repo
#   scripts/compliance.sh <dir>         check another source tree (used by --self-test)
#   scripts/compliance.sh --self-test   prove the check catches violations
set -euo pipefail

BANNED='OpenProcess|ReadProcessMemory|WriteProcessMemory|SetWindowsHookEx|SendInput|keybd_event|mouse_event|RegisterHotKey|CreateRemoteThread'
# Any HTTP method other than GET, in reqwest style or as a method constant.
NON_GET='\.(post|put|patch|delete|head)\s*\(|Method::(POST|PUT|PATCH|DELETE|HEAD|OPTIONS|CONNECT|TRACE)|"(POST|PUT|PATCH|DELETE)"'

check() {
  local root="$1" failed=0
  local dirs=()
  for d in "$root/src-tauri/src" "$root/src"; do [[ -d "$d" ]] && dirs+=("$d"); done

  if ((${#dirs[@]})) && grep -rnE --include='*.rs' --include='*.ts' --include='*.tsx' "$BANNED" "${dirs[@]}"; then
    echo "compliance: banned API found (no game-process access, no input hooks or simulation)" >&2
    failed=1
  fi

  local http="$root/src-tauri/src/lcu/http.rs"
  if [[ -f "$http" ]] && grep -nE "$NON_GET" "$http"; then
    echo "compliance: lcu/http.rs must only send GET requests" >&2
    failed=1
  fi

  return "$failed"
}

self_test() {
  local tmp
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' RETURN
  mkdir -p "$tmp/src-tauri/src/lcu"

  echo 'fn ok() {}' >"$tmp/src-tauri/src/lib.rs"
  check "$tmp" >/dev/null 2>&1 || { echo "self-test: clean tree was rejected" >&2; return 1; }

  echo 'unsafe { OpenProcess(0, false, pid) };' >"$tmp/src-tauri/src/bad.rs"
  if check "$tmp" >/dev/null 2>&1; then echo "self-test: OpenProcess was not caught" >&2; return 1; fi
  rm "$tmp/src-tauri/src/bad.rs"

  echo 'client.post(url).send().await' >"$tmp/src-tauri/src/lcu/http.rs"
  if check "$tmp" >/dev/null 2>&1; then echo "self-test: POST in lcu/http.rs was not caught" >&2; return 1; fi

  echo 'client.get(url).send().await' >"$tmp/src-tauri/src/lcu/http.rs"
  check "$tmp" >/dev/null 2>&1 || { echo "self-test: GET-only lcu/http.rs was rejected" >&2; return 1; }

  echo "compliance self-test passed"
}

case "${1:-}" in
  --self-test) self_test ;;
  "") check "$(cd "$(dirname "$0")/.." && pwd)" && echo "compliance check passed" ;;
  *) check "$1" && echo "compliance check passed" ;;
esac

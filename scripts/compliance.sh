#!/usr/bin/env bash
# Compliance guard (SPEC §2). Fails if the source uses banned Win32 APIs, or if the League client
# code writes anything other than a rune page:
#   - every file under src-tauri/src/lcu/ sends GET only, except lcu/runes.rs
#   - lcu/runes.rs (rune page import, the one allowed write) names only /lol-perks/ endpoints
#
#   scripts/compliance.sh               check the repo
#   scripts/compliance.sh <dir>         check another source tree (used by --self-test)
#   scripts/compliance.sh --self-test   prove the check catches violations
set -euo pipefail

BANNED='OpenProcess|ReadProcessMemory|WriteProcessMemory|SetWindowsHookEx|SendInput|keybd_event|mouse_event|RegisterHotKey|CreateRemoteThread'
# Any HTTP method other than GET, in reqwest style or as a method constant.
NON_GET='\.(post|put|patch|delete|head)\s*\(|Method::(POST|PUT|PATCH|DELETE|HEAD|OPTIONS|CONNECT|TRACE)|"(POST|PUT|PATCH|DELETE)"'
# League client endpoint paths named in source.
CLIENT_PATH='"/(lol-|riotclient|riot-|entitlements|player-)[^"]*'

check() {
  local root="$1" failed=0
  local dirs=()
  for d in "$root/src-tauri/src" "$root/src"; do [[ -d "$d" ]] && dirs+=("$d"); done

  if ((${#dirs[@]})) && grep -rnE --include='*.rs' --include='*.ts' --include='*.tsx' "$BANNED" "${dirs[@]}"; then
    echo "compliance: banned API found (no game-process access, no input hooks or simulation)" >&2
    failed=1
  fi

  local lcu="$root/src-tauri/src/lcu"
  if [[ -d "$lcu" ]]; then
    local others
    others=$(find "$lcu" -name '*.rs' ! -name 'runes.rs')
    if [[ -n "$others" ]] && grep -nE "$NON_GET" $others; then
      echo "compliance: League client code must only send GET (writes are allowed only in lcu/runes.rs)" >&2
      failed=1
    fi
    if [[ -f "$lcu/runes.rs" ]] && grep -noE "$CLIENT_PATH" "$lcu/runes.rs" | grep -v ':"/lol-perks/'; then
      echo "compliance: lcu/runes.rs may only use /lol-perks/ endpoints (rune page import)" >&2
      failed=1
    fi
  fi

  return "$failed"
}

self_test() {
  local tmp
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' RETURN
  mkdir -p "$tmp/src-tauri/src/lcu"
  local ok=0

  echo 'fn ok() {}' >"$tmp/src-tauri/src/lib.rs"
  check "$tmp" >/dev/null 2>&1 || { echo "self-test: clean tree was rejected" >&2; ok=1; }

  echo 'unsafe { OpenProcess(0, false, pid) };' >"$tmp/src-tauri/src/bad.rs"
  if check "$tmp" >/dev/null 2>&1; then echo "self-test: OpenProcess was not caught" >&2; ok=1; fi
  rm "$tmp/src-tauri/src/bad.rs"

  echo 'client.post(url).send().await' >"$tmp/src-tauri/src/lcu/http.rs"
  if check "$tmp" >/dev/null 2>&1; then echo "self-test: POST in lcu/http.rs was not caught" >&2; ok=1; fi
  echo 'client.get(url).send().await' >"$tmp/src-tauri/src/lcu/http.rs"
  check "$tmp" >/dev/null 2>&1 || { echo "self-test: GET-only lcu/http.rs was rejected" >&2; ok=1; }

  echo 'send(http, Method::POST, "/lol-champ-select/v1/session/actions/1", &b)' >"$tmp/src-tauri/src/lcu/champ_select.rs"
  if check "$tmp" >/dev/null 2>&1; then echo "self-test: a write outside runes.rs was not caught" >&2; ok=1; fi
  rm "$tmp/src-tauri/src/lcu/champ_select.rs"

  printf 'const PAGES: &str = "/lol-perks/v1/pages";\nsend(http, Method::POST, PAGES, &b)\n' >"$tmp/src-tauri/src/lcu/runes.rs"
  check "$tmp" >/dev/null 2>&1 || { echo "self-test: rune page import was rejected" >&2; ok=1; }

  printf 'const X: &str = "/lol-matchmaking/v1/ready-check/accept";\nsend(http, Method::POST, X, &b)\n' >"$tmp/src-tauri/src/lcu/runes.rs"
  if check "$tmp" >/dev/null 2>&1; then echo "self-test: a non-rune endpoint in runes.rs was not caught" >&2; ok=1; fi

  [[ $ok -eq 0 ]] && echo "compliance self-test passed"
  return "$ok"
}

case "${1:-}" in
  --self-test) self_test ;;
  "") check "$(cd "$(dirname "$0")/.." && pwd)" && echo "compliance check passed" ;;
  *) check "$1" && echo "compliance check passed" ;;
esac

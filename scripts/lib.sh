# Shared helpers for the scripts. Source it, don't run it.
# shellcheck shell=bash

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"

log()  { printf '\033[1;34m==>\033[0m %s\n' "$*" >&2; }
warn() { printf '\033[1;33mwarning:\033[0m %s\n' "$*" >&2; if [ -n "${GITHUB_ACTIONS:-}" ]; then echo "::warning::$*"; fi; }
die()  { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; if [ -n "${GITHUB_ACTIONS:-}" ]; then echo "::error::$*"; fi; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || die "'$1' not found. ${2:-}"; }

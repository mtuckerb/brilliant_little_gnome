#!/usr/bin/env bash
# Fail if two SQL migrations share a numeric prefix.
#
# sqlx-migrate identifies migrations by the leading 4-digit prefix. When two
# files share a prefix (e.g. `0008_one.sql` and `0008_two.sql` from PRs that
# landed concurrently), sqlx aborts at app startup on existing DBs with:
#
#     migration N was previously applied but has been modified
#
# We hit this on 2026-05-15 with the paired_devices vs custom_course_name
# collision; this script is the CI guard that prevents a repeat.
#
# Run from the `tauri/` directory; or call with the migrations dir as the
# first argument. Exits 0 on clean, 1 on duplicate prefixes (with a
# remediation hint).

set -euo pipefail

MIG_DIR="${1:-$(dirname "$0")/../src-tauri/migrations}"
MIG_DIR="$(cd "$MIG_DIR" && pwd)"

if [[ ! -d "$MIG_DIR" ]]; then
  echo "ERROR: migrations directory not found: $MIG_DIR" >&2
  exit 2
fi

# Collect filename + numeric prefix; group by prefix; report duplicates.
declare -A by_prefix
duplicate=0
max_prefix=0

while IFS= read -r -d '' file; do
  base="$(basename "$file")"
  if [[ "$base" =~ ^([0-9]+)_ ]]; then
    prefix="${BASH_REMATCH[1]}"
  else
    echo "ERROR: migration filename does not start with a numeric prefix: $base" >&2
    exit 1
  fi

  prefix_int=$((10#$prefix))
  if (( prefix_int > max_prefix )); then
    max_prefix=$prefix_int
  fi

  if [[ -n "${by_prefix[$prefix]:-}" ]]; then
    echo "ERROR: duplicate migration prefix $prefix:" >&2
    echo "       - ${by_prefix[$prefix]}" >&2
    echo "       - $base" >&2
    duplicate=1
  else
    by_prefix[$prefix]="$base"
  fi
done < <(find "$MIG_DIR" -maxdepth 1 -name '*.sql' -print0 | sort -z)

if (( duplicate )); then
  next=$(printf "%04d" $((max_prefix + 1)))
  echo "" >&2
  echo "Resolve by renumbering one of the colliding files. Next available prefix: $next" >&2
  exit 1
fi

count="${#by_prefix[@]}"
echo "OK: $count migrations with unique prefixes."

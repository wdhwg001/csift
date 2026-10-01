#!/usr/bin/env bash
# SessionStart hook (matcher "compact"): re-inject the verbatim turns the
# compaction summary clipped, one slice per registered hook.
#   usage: bash compact-slice.sh SLICE TOTAL [WINDOW]
#   SLICE 1..TOTAL, TOTAL = the number of registered copies, WINDOW chars (9000)
# Reads the hook payload on stdin. Prints one hook-output JSON object or
# nothing, and always exits 0: a hook that fails must not break the session.
# Needs jq and csift on the hook's PATH; without either it prints nothing.

slice=${1:-}
total=${2:-}
window=${3:-9000}

# Numbers only; anything else is a registration this script does not understand.
case "$slice" in ''|*[!0-9]*) exit 0 ;; esac
case "$total" in ''|*[!0-9]*) exit 0 ;; esac
case "$window" in ''|*[!0-9]*) exit 0 ;; esac
[ "$slice" -ge 1 ] 2>/dev/null || exit 0
[ "$slice" -le "$total" ] 2>/dev/null || exit 0

command -v jq >/dev/null 2>&1 || exit 0
command -v csift >/dev/null 2>&1 || exit 0
command -v cat >/dev/null 2>&1 || exit 0

input=$(cat 2>/dev/null) || exit 0
[ -n "$input" ] || exit 0

source=$(printf '%s' "$input" | jq -r '.source // empty' 2>/dev/null) || exit 0
[ "$source" = "compact" ] || exit 0

sid=$(printf '%s' "$input" | jq -r '.session_id // empty' 2>/dev/null) || exit 0
case "$sid" in
  *[!0-9a-f-]*|'') exit 0 ;;
  ????????-????-????-????-????????????) ;;
  *) exit 0 ;;
esac

chunk=$(csift verbatim "@$sid" --slices "$total" --slice "$slice" --window "$window" 2>/dev/null) || exit 0
[ -n "$chunk" ] || exit 0

header="Verbatim turns the compaction summary clipped, part $slice of $total. Same-event hooks can finish in any order, so read the parts by their numbers. The summary still owns the task state; these are the original words, each with its transcript line."
jq -n --arg c "$header
$chunk" '{hookSpecificOutput:{hookEventName:"SessionStart",additionalContext:$c}}' 2>/dev/null || exit 0
exit 0

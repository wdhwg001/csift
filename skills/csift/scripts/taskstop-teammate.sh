#!/usr/bin/env bash
# PostToolUseFailure hook (matcher "TaskStop"): when a TaskStop aimed at a
# teammate fails, tell the model whether the teammate is still running and,
# if it is, which calls address it.
#   usage: bash taskstop-teammate.sh
# Reads the hook payload on stdin. Prints one hook-output JSON object or
# nothing, and always exits 0. Read-only: it only runs `csift agents`.
# Needs jq and csift on the hook's PATH; without either it prints nothing.

command -v jq >/dev/null 2>&1 || exit 0
command -v csift >/dev/null 2>&1 || exit 0
command -v cat >/dev/null 2>&1 || exit 0

input=$(cat 2>/dev/null) || exit 0
[ -n "$input" ] || exit 0

tool=$(printf '%s' "$input" | jq -r '.tool_name // empty' 2>/dev/null) || exit 0
[ "$tool" = "TaskStop" ] || exit 0

id=$(printf '%s' "$input" | jq -r '.tool_input.task_id // .tool_input.shell_id // empty' 2>/dev/null) || exit 0
[ -n "$id" ] || exit 0

sid=$(printf '%s' "$input" | jq -r '.session_id // empty' 2>/dev/null) || exit 0
case "$sid" in
  *[!0-9a-f-]*|'') exit 0 ;;
  ????????-????-????-????-????????????) ;;
  *) exit 0 ;;
esac

rows=$(csift agents "@$sid" --shape teammate --format json 2>/dev/null) || exit 0
[ -n "$rows" ] || exit 0

# Teammates the failed id could mean: its transcript id, its name, or its Name@Team form.
matches=$(printf '%s\n' "$rows" | jq -c --arg id "$id" '
  select(.kind == "agent" and .shape == "teammate")
  | select(.agent_id == $id or .name == $id or .routing_id == $id)
  | {agent_id, name, routing_id, status}' 2>/dev/null) || exit 0
[ -n "$matches" ] || exit 0

count=$(printf '%s\n' "$matches" | jq -s 'length' 2>/dev/null) || exit 0
agent=$(printf '%s\n' "$matches" | jq -r -s '.[0].agent_id // empty' 2>/dev/null) || exit 0
name=$(printf '%s\n' "$matches" | jq -r -s '.[0].name // empty' 2>/dev/null) || exit 0
routing=$(printf '%s\n' "$matches" | jq -r -s '.[0].routing_id // empty' 2>/dev/null) || exit 0
# Named lane_state, not status: zsh refuses to assign its read-only `status`.
lane_state=$(printf '%s\n' "$matches" | jq -r -s '.[0].status // "unknown"' 2>/dev/null) || exit 0
ids=$(printf '%s\n' "$matches" | jq -r -s 'map("\(.agent_id) (\(.status // "unknown"))") | join(", ")' 2>/dev/null) || exit 0
[ -n "$name" ] && [ -n "$agent" ] || exit 0

# Teammates in this session sharing the matched name: the name forms cannot tell them apart,
# even when the failed id was one teammate's unique transcript id.
same=$(printf '%s\n' "$rows" | jq -s --arg n "$name" '[.[] | select(.kind == "agent" and .shape == "teammate" and .name == $n)] | length' 2>/dev/null) || exit 0
case "$same" in ''|*[!0-9]*) exit 0 ;; esac
nameforms="TaskStop \"$name\""
[ -n "$routing" ] && nameforms="$nameforms, TaskStop \"$routing\""
namefail="$nameforms and SendMessage to \"$name\" cannot pick one of them; TaskStop takes only the name or Name@Team, never a transcript id, so no TaskStop call can. The one call that picks one is SendMessage with to set to that teammate's transcript id (a<Name>-<hex>)"

preview="csift agents @$sid --agent @$agent"
if [ "$count" -gt 1 ]; then
  ctx="TaskStop did not resolve \"$id\". csift finds $count teammates it could mean: $ids. They share a name, so $namefail. Ask the user which teammate to stop, and show them each one with csift agents @$sid --agent @<transcript id>, before any call."
else
  case "$lane_state" in
    completed|failed|killed)
      ctx="TaskStop did not resolve \"$id\": csift reads teammate \"$name\" ($agent) as $lane_state, so it has already ended; no message can stop it, and a SendMessage addressed to its transcript id $agent would resume it instead. $preview shows its last activity." ;;
    *)
      send="SendMessage with to \"$name\" (never a to carrying @, which SendMessage refuses) and message {\"type\":\"shutdown_request\",\"reason\":\"<why>\"}"
      gate="irreversible: stopping a teammate ends its work. Preview: $preview. Only after the user's reply to that preview says go (the request that opened this task is not that reply):"
      if [ "$same" -gt 1 ]; then
        # The failed id names one of several same-named teammates: only its transcript id picks it.
        ctx="TaskStop did not resolve \"$id\" for teammate \"$name\" ($agent, csift reads it as $lane_state). $same teammates in this session share the name \"$name\", so $namefail. $gate SendMessage with to \"$agent\" and message {\"type\":\"shutdown_request\",\"reason\":\"<why>\"}."
      elif [ "$id" = "$agent" ]; then
        # The transcript id failed: TaskStop's name forms are still untried.
        stop="TaskStop with task_id \"$name\""
        [ -n "$routing" ] && stop="$stop or \"$routing\""
        ctx="TaskStop did not resolve \"$id\" for teammate \"$name\" (csift reads it as $lane_state): that is the transcript id, never a task id. TaskStop takes a teammate's name or Name@Team from Claude Code 2.1.198. $gate $stop, or $send."
      else
        # The name or Name@Team form itself failed: another TaskStop fails the same way, so the advice is SendMessage alone.
        ctx="TaskStop did not resolve \"$id\" for teammate \"$name\" (csift reads it as $lane_state), though that is a form TaskStop takes from Claude Code 2.1.198, so repeating a TaskStop call will fail the same way; stop it with SendMessage instead. $gate $send."
      fi ;;
  esac
fi

jq -n --arg c "$ctx" '{hookSpecificOutput:{hookEventName:"PostToolUseFailure",additionalContext:$c}}' 2>/dev/null || exit 0
exit 0

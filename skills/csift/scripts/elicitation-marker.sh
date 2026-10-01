#!/usr/bin/env bash
# Hook for PreToolUse, PostToolUse and PostToolUseFailure (matcher
# "AskUserQuestion|ExitPlanMode") and for Elicitation and ElicitationResult:
# records each pending and resolved question in elicitations.jsonl inside the
# session's sidecar directory, which csift merges into search, show, verbatim
# and list. That directory is the transcript's path without `.jsonl`: the
# transcript is the payload's transcript_path when it ends in
# <session-uuid>.jsonl and exists, else the one <session-uuid>.jsonl under
# ${CLAUDE_CONFIG_DIR:-$HOME/.claude}/projects/*/.
#   usage: bash elicitation-marker.sh
# Reads the hook payload on stdin. Prints nothing, always exits 0, and writes
# nothing unless the payload names a known event, a uuid session and a key.
# An ElicitationResult closes the open MCP marker whose key is its
# elicitation_id; when no open marker has that key, or the result carries no
# id, it closes every open MCP marker of its mcp_server_name, each under that
# marker's own key.

# Run under zsh by mistake, an unmatched glob below would abort; make it expand to nothing.
[ -n "${ZSH_VERSION:-}" ] && setopt NULL_GLOB 2>/dev/null

for tool in jq cat date mkdir dirname; do
  command -v "$tool" >/dev/null 2>&1 || exit 0
done

input=$(cat 2>/dev/null) || exit 0
[ -n "$input" ] || exit 0

field() { printf '%s' "$input" | jq -r "$1 // empty" 2>/dev/null; }

event=$(field '.hook_event_name') || exit 0
tool_name=$(field '.tool_name') || exit 0

kind=""; phase=""
case "$event" in
  PreToolUse)
    case "$tool_name" in AskUserQuestion|ExitPlanMode) kind=$tool_name; phase=pending ;; esac ;;
  PostToolUse|PostToolUseFailure)
    case "$tool_name" in AskUserQuestion|ExitPlanMode) kind=$tool_name; phase=resolved ;; esac ;;
  Elicitation) kind=mcp-elicitation; phase=pending ;;
  ElicitationResult) kind=mcp-elicitation; phase=resolved ;;
esac
[ -n "$kind" ] || exit 0

sid=$(field '.session_id') || exit 0
case "$sid" in
  *[!0-9a-f-]*|'') exit 0 ;;
  ????????-????-????-????-????????????) ;;
  *) exit 0 ;;
esac

# The key pairs a pending marker with its resolution; with none, pairing is impossible.
eid=""
if [ "$kind" = "mcp-elicitation" ]; then
  eid=$(field '.elicitation_id') || exit 0
  key=$(field '.elicitation_id // .mcp_server_name') || exit 0
else
  key=$(field '.tool_use_id') || exit 0
fi
[ -n "$key" ] || exit 0
server=$(field '.mcp_server_name') || exit 0

# The sidecar sits beside the session's own transcript: prefer the payload's
# transcript_path when it names that transcript and the file exists, else the
# one projects dir holding it. A path to no file is refused, never created.
transcript=$(field '.transcript_path') || exit 0
case "$transcript" in
  */"$sid".jsonl) [ -f "$transcript" ] || exit 0 ;;
  *)
    home=${CLAUDE_CONFIG_DIR:-$HOME/.claude}
    transcript=""
    for candidate in "$home"/projects/*/"$sid".jsonl; do
      [ -f "$candidate" ] || continue
      [ -z "$transcript" ] || exit 0  # two copies: the right one is unknowable
      transcript=$candidate
    done
    [ -n "$transcript" ] || exit 0 ;;
esac
sidecar="${transcript%.jsonl}/elicitations.jsonl"

ts=$(date -u +%Y-%m-%dT%H:%M:%S.000Z 2>/dev/null) || exit 0
uuid="csift-$$-$ts"
if command -v uuidgen >/dev/null 2>&1; then
  uuid=$(uuidgen 2>/dev/null | tr 'A-Z' 'a-z' 2>/dev/null) || uuid="csift-$$-$ts"
fi

# An MCP result closes the open marker keyed by its elicitation_id; when no
# open marker has that key, or the result names none (an elicitation that
# carried no id was keyed by its server), it closes every marker of its server
# still open in the sidecar, each under that marker's own key.
if [ "$phase" = "resolved" ] && [ "$kind" = "mcp-elicitation" ]; then
  [ -f "$sidecar" ] || exit 0
  records=$(jq -R -s -c --arg eid "$eid" --arg srv "$server" --arg sid "$sid" --arg ts "$ts" --arg u "$uuid" '
    [split("\n")[] | fromjson? | objects] as $all
    | [$all[] | select(.csiftPhase == "resolved") | .csiftKey] as $done
    | [$all[] | select(.csiftPhase == "pending" and .csiftKind == "mcp-elicitation")
        | select(.csiftKey | type == "string")
        | select(. as $m | any($done[]; . == $m.csiftKey) | not)] as $open
    | [$open[] | select($eid != "" and .csiftKey == $eid) | .csiftKey] as $byid
    | (if ($byid | length) > 0 then $byid
       elif $srv != "" then [$open[] | select(.csiftMcpServer == $srv) | .csiftKey]
       else [] end)
    | unique | to_entries[]
    | {type:"csift-elicitation-resolved", uuid:(if .key == 0 then $u else $u + "-" + (.key | tostring) end),
       timestamp:$ts, sessionId:$sid, csift:"elicitation-marker-v1", csiftPhase:"resolved",
       csiftKind:"mcp-elicitation", csiftKey:.value}' "$sidecar" 2>/dev/null) || exit 0
  [ -n "$records" ] || exit 0
  printf '%s\n' "$records" >>"$sidecar" 2>/dev/null || exit 0
  exit 0
fi

mkdir -p "$(dirname "$sidecar")" 2>/dev/null || exit 0

if [ "$phase" = "resolved" ]; then
  record=$(jq -cn --arg k "$kind" --arg key "$key" --arg sid "$sid" --arg ts "$ts" --arg u "$uuid" \
    '{type:"csift-elicitation-resolved", uuid:$u, timestamp:$ts, sessionId:$sid,
      csift:"elicitation-marker-v1", csiftPhase:"resolved", csiftKind:$k, csiftKey:$key}' 2>/dev/null) || exit 0
elif [ "$kind" = "mcp-elicitation" ]; then
  message=$(field '.message') || exit 0
  mode=$(field '.mode') || exit 0
  # The payload itself is not copied: a reader needs the keys below, and a copy doubles the line.
  record=$(jq -cn --arg key "$key" --arg sid "$sid" --arg ts "$ts" --arg u "$uuid" --arg ev "$event" \
    --arg tp "$transcript" --arg eid "$eid" --arg srv "$server" \
    --arg content "MCP elicitation [$server] (${mode:-elicitation}): $message" \
    '{type:"system", subtype:"mcp_elicitation", uuid:$u, timestamp:$ts, sessionId:$sid, isSidechain:false,
      content:$content, csift:"elicitation-marker-v1", csiftPhase:"pending", csiftKind:"mcp-elicitation",
      csiftKey:$key, csiftMcpServer:$srv, csiftElicitationId:(if $eid == "" then null else $eid end),
      csiftHookEvent:$ev, csiftTranscriptPath:$tp}' 2>/dev/null) || exit 0
else
  # tool_input stays whole: it is the question csift renders for the pending unit.
  record=$(printf '%s' "$input" | jq -c --arg k "$kind" --arg key "$key" --arg sid "$sid" --arg ts "$ts" --arg u "$uuid" \
    --arg ev "$event" --arg tp "$transcript" \
    '{type:"assistant", uuid:$u, timestamp:$ts, sessionId:$sid, isSidechain:false,
      message:{role:"assistant", stop_reason:"tool_use",
               content:[{type:"tool_use", id:$key, name:$k, input:(.tool_input // {})}]},
      csift:"elicitation-marker-v1", csiftPhase:"pending", csiftKind:$k, csiftKey:$key,
      csiftHookEvent:$ev, csiftTranscriptPath:$tp}' 2>/dev/null) || exit 0
fi
[ -n "$record" ] || exit 0

printf '%s\n' "$record" >>"$sidecar" 2>/dev/null || exit 0
exit 0

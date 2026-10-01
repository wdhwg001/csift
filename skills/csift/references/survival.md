# Survival: drafts, rewinds, forks, and the history above a compaction

Read this before reasoning about records the conversation no longer reaches (recalled drafts, rewound turns, replayed copies), before reading `survival`, `[abandoned]`, `[rewound]`, `pre-cut` or a crossing footer, before raising `--cross-compactions`, and before reading `show --branch-points`.

## Contents

- A transcript is not the conversation
- What each surface does with an abandoned record
- Crossing a compaction: --cross-compactions
- Floor causes and the footer
- Fork points: show --branch-points

## A transcript is not the conversation

Claude Code rebuilds a session by loading its records into a map by uuid (a later copy of a uuid wins), picking one leaf record, and walking `parentUuid` from it back to the start; only what that walk reaches is the conversation. csift asks the same of every record and gives one of three verdicts, JSON `survival` on every row (docs: `csift search --help`):

| verdict | means | how it shows |
|---|---|---|
| `live` | on the conversation | nothing |
| `abandoned` | the walk no longer reaches it: a prompt recalled and re-typed (`user.unsent`), a turn rewound past (`user.rewound`), everything below either | `[abandoned]` or `[rewound]` in search and show; JSON `abandoned_root_line` names the branch head |
| `pre-cut` | above the chain floor, the point where csift's walk stopped (usually a compaction boundary) | JSON only on search and show; `[pre-cut]` on files and image rows |

An abandoned record is outside turn numbering (its `turn_index` is null), skipped by a bare role or category, and reached by a glob or an exact leaf. A pre-cut record is reached by every selector, because what the model saw at the time is exactly what an archive answers. A rendered draft states its distance from the message that replaced it: `differs from the sent message in N chars (P% of the final)`, N counting insertions plus deletions of a shortest character edit, so P can pass 100 when the draft was longer; `identical to the sent message` when nothing changed; a rewound opener leads with `rewound: the conversation continued from L<n> instead` (measured: `csift show TARGET --line <draft line>`). JSON carries `superseding_line`, `superseding_uuid`, `diff_chars`, `diff_pct` and `diff_exact` (false when a bound stopped the comparison and both numbers are lower bounds) on the search exchange row and on the show record row.

Whether a draft was recalled is decided from the later record that replaced it, so no hook running when a prompt is submitted can tell a draft from a real message: at that moment the replacing message is the one being submitted.

The search and show footers name every count: superseded drafts, abandoned records, rewound turns, replay copies, the compaction cut line, and which rule chose the leaf (`(chain leaf: last-prompt)`); JSON `superseded_drafts`, `abandoned_records`, `rewound_turns`, `replay_copies`, `boundary_cut_line`, `leaf_source`. A replay copy, the earlier copy of a record whose uuid a later line carries again, is marked `[replay copy of L<n>]` (JSON `replay_copy_of`) and takes the verdict of the region it sits in, which is how to tell one apart: `replay_copy_of`, never `survival` (measured: 740 of 740 replay copies on one session read `pre-cut`). It stays addressable and is matched and counted at both of its lines, so a census counts it twice. Turn numbers are derived from the walk, so a rewind, a replay or a change of `--cross-compactions` renumbers the turns after it (references/ranges.md, Turns).

## What each surface does with an abandoned record

| surface | abandoned record | under `--turn` |
|---|---|---|
| `search`, `show` | reached by a glob or an exact leaf, and by a `show --line`/`--uuid` address, which renders it under a `draft (superseded — outside turn numbering)` or rewound head (measured: `csift show TARGET --line <draft line>`) | left out: it belongs to no numbered turn |
| `image` | listed and extractable, marked `[abandoned]`: the bytes are on disk | left out |
| `files`, `recover` | kept and marked: an Edit on a rewound branch did change the disk. Its turn slot reads `abandoned (root L<n>)` | admitted by the live turn it follows, so a window never hides a write inside it |
| `verbatim` | never replayed, counted in `abandoned  N turn(s) off the surviving conversation, not replayed: L...` with the `show` command that fetches one | |
| `stats` | counted in `abandoned_turns`, `rewound_turns`, `replay_copies`, whole-file figures that no `--turn` or time window touches and a change of `--cross-compactions` can move (measured on one session: `abandoned_turns` 21 at 0, 22 at 3) | |

## Crossing a compaction: --cross-compactions

A compaction boundary has a null `parentUuid`, so Claude Code's own walk stops there and a resumed session sees only the summary and the records it kept. The boundary names the record it re-anchored onto in `logicalParentUuid`, and csift's walk follows that link across up to N boundaries: `--cross-compactions N` (default 3; `all` for every boundary; `0` for Claude Code's own stop). It is a global flag, taken before or after the subcommand, and it matters on `search`, `show`, `stats`, `files`, `recover`, `verbatim` and `image` (docs: `csift --help`). Above a crossed boundary records resolve as `live` or `abandoned` by the ordinary rule, which is how a turn rewound past above a compaction gets named; above the floor they stay `pre-cut`. `verbatim` replays pre-cut turns too, so raising the budget can change the `survival` a replayed turn carries while the number of replayed turns stays the same (measured on one session: `csift --cross-compactions 0 verbatim @<uuid> --format json` replayed 61 live and 19 pre-cut turns, the same at `3` replayed 80 live).

At a non-zero budget `search`, `show`, `verbatim`, `files`, `recover` and `image` state what they crossed in up to two footer lines, while `stats` puts it in its `chain` row instead, `compaction boundaries crossed 0/8 (budget 3)` (measured: each command on one compacted session); their JSON summaries carry six keys: `crossing_budget` (a number or `all`), `boundaries_crossed`, `budget_exhausted`, `boundaries_above_floor`, `floor_line`, `floor_cause`. `search -c` and `-l` state nothing about crossing, `-c --format json` included (measured: `csift search P @<uuid> --no-subagents -c`, `-l`, and `-c --format json` on a compacted session). At `0` csift says nothing about crossing: no line, no key. `floor_line` and `floor_cause` describe one transcript and read null once several transcripts are summed (measured: `csift image @<uuid> --format json` over 3 transcripts gave `floor_cause:null`).

Where the footer lines go depends on what stdout carries. A rendered report prints them on stdout, `show` under `--line`, `--uuid` and `--turn` alike. Where stdout is data they go to stderr: `search --count-by`, `search --raw`, `recover`'s default restore, and `show --turn N --raw`. A raw search stream has no envelope, so it carries no crossing key, and at `0` it prints nothing on either stream (docs: `csift search --help`, `--raw`; measured: `csift search compaction @<uuid> --no-subagents --raw >out 2>err` on a compacted session wrote 1,481 jsonl lines to `out`, none of them a footer, and to `err` `(compaction crossing: 0 boundary(s) stepped over under --cross-compactions 3; 54 above the chain floor were not)` and the floor line; the same command under `--cross-compactions 0` left `err` empty). `show --line N --raw` and `show --uuid U --raw` print no crossing lines at all, because an address needs no walk to find its lines (docs: `csift show --help`), while the same address with `--format json` still carries the six keys, since the record's own `survival` depends on the budget (measured on a compacted session with 0 boundaries crossed and 5 above the floor: `csift show @<uuid> --turn 3 --raw` wrote the two lines to stderr, `--line 1..2 --raw` and `--uuid U --raw` wrote nothing there, and all three addresses printed the lines on stdout when rendered and carried the keys under `--format json`).

## Floor causes and the footer

The first footer line reads `(compaction crossing: N boundary(s) stepped over under --cross-compactions B[, exhausted]; M above the chain floor were not)`; when M is not zero a second line names the floor's line, its cause and whether a larger budget moves it. It offers a flag value only when the budget stopped the walk AND the boundary it stopped at still names a record the file carries, and even then it promises only that the boundaries are tried (`to try them: --cross-compactions K (or all)`); otherwise it says `raising the budget reaches none of them` or names the door's reason (measured: `csift show @<uuid> --turn -1`).

| `floor_cause` | the walk stopped at | a bigger budget moves it |
|---|---|---|
| `budget-exhausted` | a boundary the budget would not cross | yes, when that boundary's `logicalParentUuid` is a record this file carries |
| `unresolvable-boundary` | a boundary whose `logicalParentUuid` is on no line of this file | no |
| `relink-removed` | a boundary whose `logicalParentUuid` is on a line you can open, which the load-time relink took out of the map | no |
| `boundary-loop` | a boundary whose `logicalParentUuid` names a record the walk already reached | no |
| `head` | the conversation's own first record | no, the walk ended below every boundary |
| `dangling-parent` | a `parentUuid` naming no record here | no |
| `cycle` | parent links that loop | no |
| `no-leaf` | nothing resolved | no |

Above the floor csift still calls an opener a draft when a later opener shares its parent, but a crossing can name drafts that rule misses, so draft and person counts move with the budget (measured on one session, `csift --cross-compactions B search '' @<uuid> --no-subagents -t 'user.*' --count-by label` at 0 and 3: `user.message` 102 to 101, `user.unsent` 20 to 21).

## Fork points: show --branch-points

`csift show TARGET --branch-points` lists every record with more than one conversation child (a rewind, a retry, or parallel lanes), widest gap between the children first: a rewind usually shows a wide gap, a parallel lane almost none. Tool-result carriers, isMeta records and compaction summaries never count as children, so a parallel tool fan-out is no fork. Each fork names its live child (`live child: L<n>`, or `none` above the floor) and gives every child one verdict: `live`, `rewound` (an opener answered and then rewound past), `draft` (an opener never answered), `abandoned` (off the chain and not an opener, such as a retried assistant record), `pre-cut`. The parent can be any record, so a fork under a hook's attachment line prints that line and its type; `parent uuid not in this file` means exactly that. Each fork ends with a `show --line` command at its latest child.

JSON: `branch-point` rows `{uuid, line, parent_line, parent_type, live_child_line, children, widest_gap_seconds}`, `line` equal to `parent_line`, `live_child_line` null unless exactly one child is live; each child `{line, uuid, record_type, survival, verdict, ts_utc, ts_local}`; the summary `{branch_points, conversation_records, skipped_lines}` plus the crossing keys (measured: `csift show @<uuid> --branch-points --format json`).

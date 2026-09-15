//! The RESUME REPAIR PAIR: `harness.resume.prompt` + `harness.resume.placeholder`.
//!
//! When Claude Code resumes a transcript whose last loaded record is a `type:"user"` one -
//! an esc-recalled draft never resent, or any prompt that drew no reply - the LOADER repairs
//! the dangling tail before the conversation is handed to the model. It appends a
//! [`RESUME_PROMPT_MARKER`] user record (`isMeta`), and it splices a stand-in assistant
//! record carrying [`RESUME_PLACEHOLDER_TEXT`] in right after the trailing user record. Both
//! carry one identical timestamp; both are sent to the model; the splice is suppressed only
//! under Claude Code's `--reply-on-resume` flag.
//!
//! ONE writer produces every placeholder, and the two gates differ, which is why the pair is
//! not always a pair: the PROMPT is pushed only when the tail classifier returns
//! `interrupted_turn`, while the SPLICE fires whenever the last non-system/non-progress
//! record is a user record. So a placeholder whose parent is an interrupt marker, a
//! `<local-command-stdout>` echo or a slash-command wrapper is the same loader text with no
//! prompt in front of it. Both forms are machinery, so both take the leaf; whether one
//! closes a PAIR rides the hit as a fact ([`Record::resume_paired`]).
//!
//! The predicates below mirror Claude Code's own recognisers rather than approximating them,
//! with one deliberate widening: Claude Code compares the prompt content EXACTLY against the
//! env-resolved `CLAUDE_CODE_RESUME_PROMPT` value (or the bare constant), and csift cannot
//! observe the receiver's environment, so it matches the constant as a PREFIX - which covers
//! the bare form and both variants Claude Code sets for itself.

use super::*;

impl Record {
    /// The record's content as Claude Code's own content reader sees it: a bare string, or
    /// the text of a SINGLE text block. `None` for a multi-block body, a non-text block, or
    /// no content - the recogniser this mirrors treats all three as "not the marker".
    pub(crate) fn single_text_content(&self) -> Option<&str> {
        match self.message.as_ref()?.content.as_ref()? {
            Content::Text(s) => Some(s.as_str()),
            Content::Blocks(blocks) => match blocks.as_slice() {
                [Block::Text { text }] => Some(text.as_str()),
                _ => None,
            },
        }
    }

    /// True for the loader's resume repair PROMPT (`harness.resume.prompt`): an `isMeta`
    /// `type:"user"` record whose textual body opens with [`RESUME_PROMPT_MARKER`].
    ///
    /// `isMeta` is REQUIRED, and it is the half that keeps a human safe. Claude Code's own
    /// recogniser tests it, and it is the authorship flag the injecting call site stamps - so
    /// without it a person who begins a real message with the sentence ("Continue from where
    /// you left off. and also fix the tests") would have their prompt reclassified as
    /// machinery and dropped from turn numbering. With it, that message stays `user.message`
    /// and opens its turn.
    ///
    /// Mirrors the arm [`Record::classify_user_string`] takes, so the two can never disagree
    /// about which records carry the leaf: a tool_result carrier and a compaction summary are
    /// excluded exactly as `classify_user` excludes them, and the head test sees the same
    /// text. No earlier classify arm can shadow it - none of the interrupt, stdout,
    /// slash-wrapper or section markers can open with this sentence.
    ///
    /// This runs once per record of every scanned file, so it never builds the joined body
    /// [`Record::raw_message_text`] would allocate. The equivalence is exact: the joined form
    /// is the text blocks separated by newlines, the marker contains no newline, and
    /// `trim_start` can only cross whitespace-only leading blocks and their separators - so
    /// the FIRST block with any non-whitespace content decides, exactly as the joined string
    /// would.
    #[must_use]
    pub fn is_resume_prompt(&self) -> bool {
        if !self.is_type("user")
            || !self.is_meta.unwrap_or(false)
            || self.is_compact_summary.unwrap_or(false)
        {
            return false;
        }
        let Some(content) = self.message.as_ref().and_then(|m| m.content.as_ref()) else {
            return false;
        };
        match content {
            Content::Text(s) => s.trim_start().starts_with(RESUME_PROMPT_MARKER),
            Content::Blocks(blocks) => {
                if blocks.iter().any(|b| matches!(b, Block::ToolResult { .. })) {
                    return false;
                }
                for b in blocks {
                    let Block::Text { text } = b else { continue };
                    let head = text.trim_start();
                    if head.is_empty() {
                        continue;
                    }
                    return head.starts_with(RESUME_PROMPT_MARKER);
                }
                false
            }
        }
    }

    /// True for the loader's resume repair PLACEHOLDER (`harness.resume.placeholder`).
    ///
    /// A 1:1 port of Claude Code's own recogniser: an `assistant` record that is NOT an
    /// API-error notice, whose `message.model` is the [`SYNTHETIC_MODEL`] sentinel, and whose
    /// single-text content is exactly [`RESUME_PLACEHOLDER_TEXT`]. There is no parent
    /// condition, because the producer has none.
    ///
    /// The model sentinel is the load-bearing half: it is what separates a record Claude Code
    /// fabricated from one the assistant produced, so a match here is never the model's text.
    #[must_use]
    pub fn is_resume_placeholder(&self) -> bool {
        self.is_type("assistant")
            && self.is_api_error_message != Some(true)
            && self.message.as_ref().and_then(Message::model_id) == Some(SYNTHETIC_MODEL)
            && self.single_text_content() == Some(RESUME_PLACEHOLDER_TEXT)
    }

    /// Whether this placeholder CLOSES a repair pair - `Some(true)` when its `parentUuid` is
    /// a [`Class::ResumePrompt`] record in the same transcript, `Some(false)` when it is not.
    ///
    /// `None` means the question does not apply or cannot be answered: the record is not a
    /// placeholder, or no prompt index was supplied (a bare [`ClassifyCtx`] - never the case
    /// on a real scan, which builds the index per file). Never guessed: an absent index
    /// yields no verdict rather than a fabricated `false`.
    ///
    /// Claude Code pairs the two by ADJACENCY in its loaded array; on disk the appended
    /// prompt is the placeholder's `parentUuid`, which is the same relation csift can observe
    /// from a transcript alone.
    #[must_use]
    pub fn resume_paired(&self, ctx: &ClassifyCtx) -> Option<bool> {
        if !self.is_resume_placeholder() {
            return None;
        }
        let prompts = ctx.resume_prompt_uuids?;
        Some(
            self.parent_uuid
                .as_deref()
                .is_some_and(|p| prompts.contains(p)),
        )
    }
}

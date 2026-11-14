// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The review of a pending change: its text and the completeness gate.
//! [`crate::Session`] builds it after every dispatch and refuses Accept
//! while it is incomplete, so no front end can accept a change whose
//! review was cut short. Front ends lay the lines out; the text and the
//! limits are the engine's.
use tilewright_editor_commands::Preview;
use tilewright_editor_document::terminal_field;

/// Most bytes of source excerpt rows in one review.
pub const MAXIMUM_REVIEW_BYTES: usize = 64 << 10;
/// Most changed files one review shows; a change with more is incomplete.
pub const MAXIMUM_REVIEW_FILES: usize = 32;
/// Separate allowance for review notes and deletion warnings, which are
/// already bounded. Warnings past it are omitted with a line saying so;
/// they never make a review incomplete.
pub const REVIEW_WARNING_BYTES: usize = 16 << 10;
const REVIEW_CONTEXT_BYTES: usize = 24;
const REVIEW_ROW_BYTES: usize = 256;
/// The line that ends the warnings when some did not fit.
pub const REVIEW_WARNINGS_OMITTED: &str = "Further warnings are omitted from this review.";
/// The first line of an incomplete review, and the message Accept is
/// refused with.
pub const REVIEW_LIMIT_MESSAGE: &str = "Change exceeds the visible review limit. Cancel this edit.";
/// The review when nothing is pending.
pub const NOTHING_PENDING: &str = "No pending changes. Edits are reviewed before saving.";

/// Review lines for the pending change, warnings first, then a bounded
/// excerpt of every changed file. Incomplete when a file or the excerpt
/// budget did not fit; then its first line is [`REVIEW_LIMIT_MESSAGE`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Review {
    pub lines: Vec<String>,
    pub complete: bool,
    /// How many of the pending warnings the lines include; the rest are
    /// omitted.
    pub warnings_shown: usize,
}

impl Review {
    /// The review of `pending` with its notes and warnings.
    #[must_use]
    pub fn of(pending: Option<&Preview>, warnings: &[String]) -> Self {
        let Some(plan) = pending else {
            return Self {
                lines: vec![NOTHING_PENDING.into()],
                complete: true,
                warnings_shown: 0,
            };
        };
        let mut review = Self {
            lines: Vec::new(),
            complete: true,
            warnings_shown: 0,
        };
        let mut remaining = MAXIMUM_REVIEW_BYTES;
        // Deletion warnings come first, with their own allowance outside the
        // diff budget: they inform the author and never block Accept.
        let mut allowance = REVIEW_WARNING_BYTES;
        for warning in warnings {
            let warning = terminal_field(warning);
            if warning.len() > allowance {
                review.lines.push(REVIEW_WARNINGS_OMITTED.into());
                break;
            }
            allowance -= warning.len();
            review.lines.push(warning);
            review.warnings_shown += 1;
        }
        for (index, file) in plan.changed().enumerate() {
            if index >= MAXIMUM_REVIEW_FILES {
                review.complete = false;
                break;
            }
            review
                .lines
                .push(format!("{} / source excerpt; scroll for more", file.path));
            if !append_change_review(
                &mut review.lines,
                file.before.as_deref().unwrap_or("<absent>"),
                file.after.as_deref().unwrap_or("<absent>"),
                &mut remaining,
            ) {
                review.complete = false;
                break;
            }
        }
        if !review.complete {
            review.lines.insert(0, REVIEW_LIMIT_MESSAGE.into());
        }
        review
    }
}

/// Read-only display ranges from exact prepared images; no source parsing or writing.
fn changed_context(before: &str, after: &str) -> (std::ops::Range<usize>, std::ops::Range<usize>) {
    let mut prefix = before
        .bytes()
        .zip(after.bytes())
        .take_while(|(a, b)| a == b)
        .count();
    while !before.is_char_boundary(prefix) || !after.is_char_boundary(prefix) {
        prefix -= 1;
    }
    let mut suffix = before[prefix..]
        .bytes()
        .rev()
        .zip(after[prefix..].bytes().rev())
        .take_while(|(a, b)| a == b)
        .count();
    while !before.is_char_boundary(before.len() - suffix)
        || !after.is_char_boundary(after.len() - suffix)
    {
        suffix -= 1;
    }
    let context = |text: &str| {
        let mut start = prefix.saturating_sub(REVIEW_CONTEXT_BYTES);
        while !text.is_char_boundary(start) {
            start -= 1;
        }
        let mut end = (text.len() - suffix)
            .saturating_add(REVIEW_CONTEXT_BYTES)
            .min(text.len());
        while !text.is_char_boundary(end) {
            end += 1;
        }
        start..end
    };
    (context(before), context(after))
}

fn review_chunks(text: &str, range: std::ops::Range<usize>) -> Vec<String> {
    let escaped = terminal_field(&text[range.clone()]);
    let mut rows = Vec::new();
    let mut start = 0;
    while start < escaped.len() {
        let mut end = (start + REVIEW_ROW_BYTES).min(escaped.len());
        while !escaped.is_char_boundary(end) {
            end -= 1;
        }
        let prefix = if start == 0 && range.start > 0 {
            "[...] "
        } else {
            ""
        };
        let suffix = if end == escaped.len() && range.end < text.len() {
            " [...]"
        } else {
            ""
        };
        rows.push(format!("{prefix}{}{suffix}", &escaped[start..end]));
        start = end;
    }
    if rows.is_empty() {
        rows.push("<empty>".into());
    }
    rows
}

/// Append the excerpt rows of one changed file, `-` before and `+` after,
/// charging the shown bytes to `remaining`. Returns `false`, appending
/// nothing, when they do not fit.
pub fn append_change_review(
    lines: &mut Vec<String>,
    before: &str,
    after: &str,
    remaining: &mut usize,
) -> bool {
    let (old, new) = changed_context(before, after);
    let bytes = old.len().saturating_add(new.len());
    if bytes > *remaining {
        return false;
    }
    let old_rows = review_chunks(before, old);
    let new_rows = review_chunks(after, new);
    let displayed = old_rows
        .iter()
        .chain(&new_rows)
        .map(String::len)
        .sum::<usize>();
    if displayed > *remaining {
        return false;
    }
    *remaining -= displayed;
    lines.push("[...] marks unchanged source omitted from this excerpt.".into());
    for index in 0..old_rows.len().max(new_rows.len()) {
        if let Some(row) = old_rows.get(index) {
            lines.push(format!("- {row}"));
        }
        if let Some(row) = new_rows.get(index) {
            lines.push(format!("+ {row}"));
        }
    }
    true
}

/// How a pending change touches one file.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChangeKind {
    Created,
    Modified,
    Removed,
}

/// One file a pending change changes, with its sizes in bytes before and
/// after (`None` when absent).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChangedFile<'s> {
    pub path: &'s str,
    pub kind: ChangeKind,
    pub before_bytes: Option<usize>,
    pub after_bytes: Option<usize>,
}

/// A bounded, structured view of the pending change for callers that do
/// not draw the review: at most [`MAXIMUM_REVIEW_FILES`] files (with the
/// count left out), the review, the notes and warnings it shows, and the
/// revisions. World text in it is unescaped data.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingSummary<'s> {
    /// The committed source revision the change applies to.
    pub base: [u8; 32],
    /// The source revision the change publishes.
    pub result: [u8; 32],
    pub files: Vec<ChangedFile<'s>>,
    /// Changed files past [`MAXIMUM_REVIEW_FILES`], not listed.
    pub files_omitted: usize,
    /// The review lines, exactly as a person sees them.
    pub review: &'s Review,
    /// The notes and warnings the review shows, unescaped.
    pub warnings: &'s [String],
    /// Notes and warnings the review omits for its warning allowance.
    pub warnings_omitted: usize,
    /// Whether Accept may publish it: the review is complete.
    pub complete: bool,
}

impl<'s> PendingSummary<'s> {
    /// The summary of `pending`, prepared on the source revision `base`,
    /// with its `review` and the notes and warnings that review was built
    /// from. [`crate::Session::pending_summary`] is the usual way to get one.
    #[must_use]
    pub fn of(
        base: [u8; 32],
        pending: &'s Preview,
        review: &'s Review,
        warnings: &'s [String],
    ) -> Self {
        let mut files = Vec::new();
        let mut files_omitted = 0;
        for file in pending.changed() {
            if files.len() == MAXIMUM_REVIEW_FILES {
                files_omitted += 1;
                continue;
            }
            files.push(ChangedFile {
                path: &file.path,
                kind: match (&file.before, &file.after) {
                    (None, _) => ChangeKind::Created,
                    (_, None) => ChangeKind::Removed,
                    _ => ChangeKind::Modified,
                },
                before_bytes: file.before.as_ref().map(String::len),
                after_bytes: file.after.as_ref().map(String::len),
            });
        }
        let shown = review.warnings_shown.min(warnings.len());
        Self {
            base,
            result: pending.world().source_digest(),
            files,
            files_omitted,
            review,
            warnings: &warnings[..shown],
            warnings_omitted: warnings.len() - shown,
            complete: review.complete,
        }
    }
}

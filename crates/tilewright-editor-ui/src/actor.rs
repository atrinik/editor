// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Who or what makes a session's changes: the `actor` of every transaction
//! the session prepares, given once when the session is constructed.
use std::fmt;

use crate::{RefusalCode, SessionError};

/// A transaction actor, such as `editor:prototype`, `human:<login>` or
/// `agent:<client>`: 1 to [`Actor::MAXIMUM_CHARACTERS`] characters of
/// printable ASCII (U+0020 to U+007E), not starting or ending with a
/// space. Printable ASCII reads the same on every display: no control,
/// invisible, combining or right-to-left character can hide in it. A
/// display name in other scripts belongs elsewhere.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Actor(String);

impl Actor {
    /// Most characters of an actor, as `tilewright.transaction/1` allows.
    pub const MAXIMUM_CHARACTERS: usize = 256;

    /// # Errors
    /// Refuses, with [`RefusalCode::ActorInvalid`], an empty actor, one
    /// longer than [`Actor::MAXIMUM_CHARACTERS`] characters, one with a
    /// character outside printable ASCII, or one that starts or ends with
    /// a space.
    pub fn new(text: impl Into<String>) -> Result<Self, SessionError> {
        let text = text.into();
        let refused = |reason: &str| {
            Err(SessionError::new(
                RefusalCode::ActorInvalid,
                format!("actor refused: {reason}"),
            ))
        };
        if text.is_empty() {
            return refused("it is empty");
        }
        if !text.bytes().all(|byte| matches!(byte, b' '..=b'~')) {
            return refused("use printable ASCII only (U+0020 to U+007E)");
        }
        // Printable ASCII has one byte per character.
        if text.len() > Self::MAXIMUM_CHARACTERS {
            return refused("it has more than 256 characters");
        }
        if text.starts_with(' ') || text.ends_with(' ') {
            return refused("it starts or ends with a space");
        }
        Ok(Self(text))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Actor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::Actor;
    use crate::RefusalCode;

    #[test]
    fn actors_are_one_to_256_printable_ascii_characters_without_edge_spaces() {
        for text in [
            "editor:prototype",
            "agent:claude-code/1.2",
            "human:zoe rose",
            "~",
        ] {
            assert_eq!(Actor::new(text).unwrap().as_str(), text);
        }
        assert!(Actor::new("x".repeat(256)).is_ok());
        for text in [
            String::new(),
            "x".repeat(257),
            "human:Zoë".into(),
            "human:\u{3164}".into(),
            "agent:\u{202e}reversed".into(),
            "agent:zero\u{200b}width".into(),
            "agent:\u{feff}".into(),
            "agent:\u{a0}".into(),
            "agent:\nname".into(),
            "agent:\u{7f}".into(),
            " agent:leading".into(),
            "agent:trailing ".into(),
            " ".into(),
        ] {
            let refused = Actor::new(text.clone()).unwrap_err();
            assert_eq!(refused.code(), RefusalCode::ActorInvalid, "{text:?}");
            assert!(refused.message().starts_with("actor refused: "));
        }
    }
}

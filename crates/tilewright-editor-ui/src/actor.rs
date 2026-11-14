// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Who or what makes a session's changes: the `actor` of every transaction
//! the session prepares, given once when the session is constructed.
use std::fmt;

use crate::{RefusalCode, SessionError};

/// A transaction actor, such as `editor:prototype`, `human:<name>` or
/// `agent:<client>`. Valid when non-empty, at most
/// [`Actor::MAXIMUM_CHARACTERS`] characters (the transaction format's
/// limit) and made only of printable ASCII (space included) and Unicode
/// letters and digits. That leaves out control characters and every
/// invisible or format character (bidirectional overrides, zero-width
/// characters), so an actor built from a client's name reads as it is.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Actor(String);

impl Actor {
    /// Most characters of an actor, as `tilewright.transaction/1` allows.
    pub const MAXIMUM_CHARACTERS: usize = 256;

    /// # Errors
    /// Refuses, with [`RefusalCode::ActorInvalid`], an empty actor, one
    /// longer than [`Actor::MAXIMUM_CHARACTERS`] characters, or one with a
    /// character outside the safe set.
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
        if text.chars().count() > Self::MAXIMUM_CHARACTERS {
            return refused("it has more than 256 characters");
        }
        if text.chars().any(char::is_control) {
            return refused("it contains a control character");
        }
        if !text
            .chars()
            .all(|c| matches!(c, ' '..='~') || c.is_alphanumeric())
        {
            return refused(
                "use printable ASCII and Unicode letters and digits only (no invisible or format characters)",
            );
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
    fn actors_are_bounded_non_empty_and_free_of_controls() {
        assert_eq!(
            Actor::new("editor:prototype").unwrap().as_str(),
            "editor:prototype"
        );
        assert!(Actor::new("é".repeat(256)).is_ok());
        for text in [
            "human:Zoë Ångström",
            "agent:client-1.2 (test)",
            "human:日本",
        ] {
            assert!(Actor::new(text).is_ok(), "{text}");
        }
        for text in [
            String::new(),
            "x".repeat(257),
            "agent:\nname".into(),
            "agent:\u{7f}".into(),
            "agent:\u{9b}".into(),
            "agent:\u{202e}reversed".into(),
            "agent:zero\u{200b}width".into(),
            "agent:\u{feff}".into(),
            "agent:\u{2028}".into(),
            "agent:\u{a0}".into(),
        ] {
            let refused = Actor::new(text).unwrap_err();
            assert_eq!(refused.code(), RefusalCode::ActorInvalid);
            assert!(refused.message().starts_with("actor refused: "));
        }
    }
}

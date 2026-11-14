// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

#![forbid(unsafe_code)]

//! Read-only observation of a world for front ends that do not draw it:
//! the `overview`, `tile`, `catalog` and `lint` commands of the
//! `tilewright-editor` binary today, and the observe tools of the MCP
//! server later (issue #57). See `docs/CLI.md`.
//!
//! Read-only by construction: a world is opened with
//! `WorldDocument::open`, its catalog with the preview loader, and nothing
//! here holds a session, a recovery store, a transaction or any other write
//! authority: this crate never builds a `Session` and does not depend
//! directly on the transaction, history or recovery crates.
//! Every fact comes from the World, document, preview and ui crates, the
//! same functions the desktop editor draws from. Every report is bounded
//! ([`limits`]) and world text in it is data: clipped, and escaped in both
//! printed forms.

pub mod catalog;
pub mod cli;
pub mod limits;
pub mod lint;
pub mod node;
pub mod overview;
pub mod tile;
pub mod world;

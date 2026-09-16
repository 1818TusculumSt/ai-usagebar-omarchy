//! GitHub Copilot subscription quota through the undocumented
//! `copilot_internal/user` endpoint — the same one the editor extensions read
//! to draw their premium-request meter.
//!
//! There is no supported public API for an individual seat's premium-request
//! balance: the documented `/users/{u}/settings/billing/…` report covers only
//! accounts on the enhanced billing platform, and the org/enterprise Copilot
//! metrics endpoints require admin credentials a seat holder does not have.
//!
//! ai-usagebar never mints, refreshes, or stores a GitHub token. It reads one
//! that already exists — an env var the user exported, or whatever `gh` holds
//! in its own credential store — and sends it to exactly one endpoint.

pub mod creds;
pub mod fetch;
pub mod types;
pub mod vendor;

pub use fetch::{FetchOutcome, fetch_snapshot};

//! Z.AI / BigModel vendor — undocumented `/api/monitor/usage/quota/limit`.
//! Auth header is `Authorization: <KEY>` with NO `Bearer` prefix. Team
//! (enterprise) keys add `?type=2` + org headers on the CN site; pay-as-you-go
//! keys report the model-usage 7-day aggregate instead — see `fetch::FetchPlan`.

pub mod fetch;
pub mod types;
pub mod vendor;

pub use fetch::{FetchOutcome, FetchPlan, fetch_snapshot, fetch_snapshot_plan};

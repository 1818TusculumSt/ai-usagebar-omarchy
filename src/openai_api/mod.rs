//! OpenAI Admin API vendor — trailing-30-day billed dollars from the Costs
//! API (`GET /v1/organization/costs`) over a platform **Admin key**. These
//! organization endpoints reject regular project `sk-` keys, and there is no
//! balance endpoint, so this reports spend against a self-configured limit —
//! the OpenAI twin of the Anthropic Admin vendor.

pub mod fetch;
pub mod types;
pub mod vendor;

pub use fetch::{FetchOutcome, fetch_snapshot};

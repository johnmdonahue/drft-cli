//! Inactive building blocks for experimental local usage collection.
//!
//! No command calls this module. Revision-1 envelopes are constructed in memory;
//! storage can be initialized, opened, locked, and physically inventoried with
//! bounded reads, independent wire classification, and grouped retention planning.
//! Native persistence and activation remain separate work.

pub mod bounded;
pub mod capture;
pub mod event;
pub mod identity;
pub mod record;
pub mod store;

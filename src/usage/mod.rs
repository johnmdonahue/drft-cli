//! Inactive building blocks for experimental local usage collection.
//!
//! No command calls this module. Revision-1 envelopes are constructed in memory;
//! filesystem storage and activation remain separate, unqualified work.

pub mod bounded;
pub mod capture;
pub mod event;
pub mod identity;

//! Inactive building blocks for experimental local usage collection.
//!
//! No command calls this module. Event envelopes, filesystem storage, and
//! activation are separate integration work; these primitives do not qualify
//! collection or define the complete event schema.

pub mod bounded;
pub mod capture;
pub mod identity;

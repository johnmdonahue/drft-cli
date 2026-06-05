//! Sources deliver `(path, bytes)` records to builders. v0.8 ships a single
//! built-in source: [`fs`], the filesystem walk.

pub mod fs;

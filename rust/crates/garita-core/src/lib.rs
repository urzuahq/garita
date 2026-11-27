//! Pure gate logic: no I/O, no clock, no environment.

pub mod adapter;
pub mod argv;
pub mod config;
pub mod describe;
pub mod glob;
pub mod object_id;
pub mod receipt;
pub mod report;
pub mod risk;

pub use object_id::{ObjectId, ObjectIdError};
pub use report::SCHEMA_VERSION;

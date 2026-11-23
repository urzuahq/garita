//! Pure gate logic: no I/O, no clock, no environment.

pub mod adapter;
pub mod argv;
pub mod config;
pub mod receipt;
pub mod report;

pub use report::SCHEMA_VERSION;

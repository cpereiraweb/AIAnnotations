//! Queries behind each screen. Each takes a `Connection` and returns plain serializable data;
//! instants are milliseconds UTC and formatting is the frontend's job.

pub mod projects;
pub mod runs;

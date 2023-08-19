//! This crate provides access to the HiDrive HTTP API, including OAuth flow.

mod chunking;
mod gen_call;

pub mod hashing;
pub mod hidrive;
pub mod oauth2;
pub mod types;

pub use hidrive::HiDrive;

pub use oauth2::{Authorizer, ClientSecret, Credentials};

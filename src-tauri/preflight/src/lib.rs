//! Preflight engine for destination readiness checks.
//!
//! Four modules, split by what kind of thing is being asked:
//!
//! - `bridge`  — turns a manifest's requirements into engine requirements
//! - `engine`  — runs checks, caches them, and scores readiness
//! - `identity`— provider sign-in readiness
//! - `environment` / `service` — variable presence and local port checks

pub mod bridge;
pub mod engine;
pub mod environment;
pub mod identity;
pub mod service;

pub use bridge::{
    to_check_requirements, ENVIRONMENT_ADAPTER, IDENTITY_ADAPTER, SERVICE_ADAPTER,
};
pub use engine::{PreflightCheck, PreflightEngine, PreflightOptions, PreflightReport};

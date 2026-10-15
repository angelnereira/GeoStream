//! Test client, fixtures and runners for test vectors and scenarios (docs/18 §6).
#![forbid(unsafe_code)]

pub mod client;
pub mod dsl;
pub mod json_path;
pub mod matcher;
pub mod runner;
pub mod server;
pub mod template;

pub use client::{ClientError, TestStreamClient};
pub use dsl::{ActorDef, HttpExpectDef, Scenario, StepDef};
pub use json_path::{count_path, extract_path};
pub use matcher::{normalize_json_keys, partial_match, to_snake_case};
pub use runner::{ScenarioReport, run_scenario};
pub use server::TestServer;
pub use template::TemplateContext;

/// Loads a Scenario from a JSON string.
pub fn parse_scenario(json_str: &str) -> Result<Scenario, serde_json::Error> {
    serde_json::from_str(json_str)
}

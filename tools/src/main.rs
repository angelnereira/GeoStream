//! geostream-tools: Repository and specification consistency checks in pure Rust.
#![forbid(unsafe_code)]

use clap::{Parser, Subcommand};
use geostream_tools::*;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "geostream-tools")]
#[command(about = "Repository and specification consistency verification for GeoStream")]
struct Cli {
    #[arg(long, global = true)]
    repo_root: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Run all consistency checks
    All,
    /// Verify JSON syntax validity under contracts/
    Json,
    /// Verify crate dependency rules from 04_ARCHITECTURE.md §4.1
    CrateDeps,
    /// Verify ErrorCode catalog consistency across proto, OpenAPI, and docs
    ErrorCatalog,
    /// Verify GS_* and policy configuration keys against 17_CONFIGURATION.md
    ConfigKeys,
    /// Verify requirement IDs and traceability across docs
    Requirements,
    /// Verify auth matrix parity between 06 §8.2 and OpenAPI
    AuthMatrix,
    /// Verify x-gs-phase, scopes and ID prefixes against the normative docs
    Phases,
    /// Verify enum parity across Protobuf, OpenAPI and the normative docs
    Enums,
    /// Verify the documents the public portal publishes (ADR-0034 §2)
    PortalDocs,
    /// Verify the own-map style: own tiles, own fonts and ODbL attribution (FR-MAP-007)
    TilesStyle,
    /// Verify relative markdown links across the repository
    MarkdownLinks,
    /// Verify CHANGELOG.md format and optional PR update
    Changelog {
        #[arg(long)]
        base_ref: Option<String>,
    },
}

fn find_repo_root() -> PathBuf {
    if let Ok(cargo_manifest) = std::env::var("CARGO_MANIFEST_DIR") {
        let manifest_path = PathBuf::from(cargo_manifest);
        if manifest_path.file_name().and_then(|s| s.to_str()) == Some("tools") {
            return manifest_path
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or(manifest_path);
        }
        return manifest_path;
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn run_check<F>(name: &str, f: F) -> bool
where
    F: FnOnce() -> Result<(), Vec<String>>,
{
    println!("\n--- [{name}] ---");
    match f() {
        Ok(()) => true,
        Err(errors) => {
            eprintln!("FAIL: {name} failed with {} error(s):", errors.len());
            for err in errors {
                eprintln!("  - {err}");
            }
            false
        }
    }
}

fn run_all(root: &Path) -> bool {
    println!("{}", "=".repeat(70));
    println!("Running GeoStream Consistency Checks (geostream-tools)");
    println!("{}", "=".repeat(70));

    let mut all_ok = true;

    if !run_check("Contracts JSON Validity", || json::check_json(root)) {
        all_ok = false;
    }
    if !run_check("Crate Dependency Rules (04 §4.1)", || {
        crate_deps::check_crate_deps(root)
    }) {
        all_ok = false;
    }
    if !run_check("ErrorCode Catalog Consistency", || {
        error_catalog::check_error_catalog(root)
    }) {
        all_ok = false;
    }
    if !run_check("Configuration Keys Consistency (17)", || {
        config_keys::check_config_keys(root)
    }) {
        all_ok = false;
    }
    if !run_check(
        "Requirement IDs and Traceability (03, TRACEABILITY)",
        || requirements::check_requirements(root),
    ) {
        all_ok = false;
    }
    if !run_check("Auth Matrix Consistency (06 §8.2 vs OpenAPI)", || {
        auth_matrix::check_auth_matrix(root)
    }) {
        all_ok = false;
    }
    if !run_check(
        "Phases, Scopes and ID Prefixes (21, 06, 05 vs contracts)",
        || phases::check_phases_and_scopes(root),
    ) {
        all_ok = false;
    }
    if !run_check("Enum Parity (proto, OpenAPI, docs)", || {
        enums::check_enum_parity(root)
    }) {
        all_ok = false;
    }
    if !run_check("Portal Published Docs (ADR-0034 §2)", || {
        portal_docs::check_portal_docs(root)
    }) {
        all_ok = false;
    }
    if !run_check("Own Map Style (FR-MAP-007)", || {
        tiles_style::check_tiles_style(root)
    }) {
        all_ok = false;
    }
    if !run_check("Markdown Relative Links", || {
        markdown_links::check_markdown_links(root)
    }) {
        all_ok = false;
    }
    if !run_check("Changelog Format (19 §8)", || {
        changelog::check_changelog(root, None)
    }) {
        all_ok = false;
    }

    println!("\n{}", "=".repeat(70));
    if all_ok {
        println!("SUCCESS: All consistency checks passed!");
        println!("{}", "=".repeat(70));
    } else {
        eprintln!("FAILURE: One or more consistency checks failed!");
        println!("{}", "=".repeat(70));
    }

    all_ok
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let root = cli.repo_root.unwrap_or_else(find_repo_root);

    let ok = match cli.command.unwrap_or(Commands::All) {
        Commands::All => run_all(&root),
        Commands::Json => run_check("Contracts JSON Validity", || json::check_json(&root)),
        Commands::CrateDeps => run_check("Crate Dependency Rules (04 §4.1)", || {
            crate_deps::check_crate_deps(&root)
        }),
        Commands::ErrorCatalog => run_check("ErrorCode Catalog Consistency", || {
            error_catalog::check_error_catalog(&root)
        }),
        Commands::ConfigKeys => run_check("Configuration Keys Consistency (17)", || {
            config_keys::check_config_keys(&root)
        }),
        Commands::Requirements => run_check(
            "Requirement IDs and Traceability (03, TRACEABILITY)",
            || requirements::check_requirements(&root),
        ),
        Commands::AuthMatrix => run_check("Auth Matrix Consistency (06 §8.2 vs OpenAPI)", || {
            auth_matrix::check_auth_matrix(&root)
        }),
        Commands::Phases => run_check(
            "Phases, Scopes and ID Prefixes (21, 06, 05 vs contracts)",
            || phases::check_phases_and_scopes(&root),
        ),
        Commands::Enums => run_check("Enum Parity (proto, OpenAPI, docs)", || {
            enums::check_enum_parity(&root)
        }),
        Commands::PortalDocs => run_check("Portal Published Docs (ADR-0034 §2)", || {
            portal_docs::check_portal_docs(&root)
        }),
        Commands::TilesStyle => run_check("Own Map Style (FR-MAP-007)", || {
            tiles_style::check_tiles_style(&root)
        }),
        Commands::MarkdownLinks => run_check("Markdown Relative Links", || {
            markdown_links::check_markdown_links(&root)
        }),
        Commands::Changelog { base_ref } => run_check("Changelog Format (19 §8)", || {
            changelog::check_changelog(&root, base_ref.as_deref())
        }),
    };

    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

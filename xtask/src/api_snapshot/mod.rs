//! `cargo xtask api-snapshot`: public API summaries from rustdoc JSON.
//!
//! rustdoc's JSON output is unstable (`-Z unstable-options`), so the command
//! runs the pinned stable toolchain with `RUSTC_BOOTSTRAP=1` and records the
//! `format_version` it consumed. The summary is a review aid for public API
//! changes, not a semver checker.

mod render;
mod summary;

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;

use crate::publish_order::order;
use crate::workspace::metadata_no_deps;

/// Directory of the committed summaries, relative to the workspace root.
const OUTPUT_DIR: &str = "docs/api";

/// rustdoc JSON format versions this command understands.
const SUPPORTED_FORMAT_VERSIONS: &[u64] = &[60];

pub(crate) fn run(check: bool) -> Result<()> {
    let metadata = metadata_no_deps()?;
    let root = metadata.workspace_root.as_std_path();
    let target_dir = metadata.target_directory.as_std_path();
    let output_dir = root.join(OUTPUT_DIR);
    let mut differences = Vec::new();
    for crate_name in order()? {
        let json_path = rustdoc_json(root, target_dir, &crate_name)?;
        let text = std::fs::read_to_string(&json_path)
            .with_context(|| format!("failed to read {}", json_path.display()))?;
        let document: serde_json::Value = serde_json::from_str(&text)
            .with_context(|| format!("invalid rustdoc JSON in {}", json_path.display()))?;
        let format_version = document["format_version"]
            .as_u64()
            .context("rustdoc JSON has no format_version")?;
        if !SUPPORTED_FORMAT_VERSIONS.contains(&format_version) {
            bail!(
                "unsupported rustdoc JSON format_version {format_version} (supported: {SUPPORTED_FORMAT_VERSIONS:?}); update xtask/src/api_snapshot"
            );
        }
        let summary = summary::summarize(&crate_name, &document)?;
        let mut rendered = serde_json::to_string_pretty(&summary)?;
        rendered.push('\n');
        let output_path = output_dir.join(format!("{crate_name}.json"));
        if check {
            let existing = std::fs::read_to_string(&output_path).unwrap_or_default();
            if existing != rendered {
                differences.push(crate_name.clone());
            }
            println!(
                "{crate_name}: {} public items ({})",
                summary.items.len(),
                if existing == rendered {
                    "unchanged"
                } else {
                    "DIFFERS"
                }
            );
            if existing != rendered {
                print_differences(&existing, &rendered);
            }
        } else {
            std::fs::create_dir_all(&output_dir)
                .with_context(|| format!("failed to create {}", output_dir.display()))?;
            std::fs::write(&output_path, &rendered)
                .with_context(|| format!("failed to write {}", output_path.display()))?;
            println!(
                "{crate_name}: {} public items -> {}",
                summary.items.len(),
                output_path.display()
            );
        }
    }
    if !differences.is_empty() {
        bail!(
            "API snapshots differ for {}; run `cargo xtask api-snapshot` and review the diff",
            differences.join(", ")
        );
    }
    Ok(())
}

/// Runs `cargo rustdoc` for `crate_name` and returns the JSON file path.
fn rustdoc_json(root: &Path, target_dir: &Path, crate_name: &str) -> Result<PathBuf> {
    let status = Command::new(env_or_cargo())
        .current_dir(root)
        .env("RUSTC_BOOTSTRAP", "1")
        .args([
            "rustdoc",
            "-p",
            crate_name,
            "--lib",
            "--all-features",
            "--",
            "-Z",
            "unstable-options",
            "--output-format",
            "json",
        ])
        .status()
        .with_context(|| format!("failed to run cargo rustdoc for {crate_name}"))?;
    if !status.success() {
        bail!("cargo rustdoc failed for {crate_name}");
    }
    let file = target_dir
        .join("doc")
        .join(format!("{}.json", crate_name.replace('-', "_")));
    if !file.is_file() {
        bail!("rustdoc did not produce {}", file.display());
    }
    Ok(file)
}

/// `$CARGO` when cargo invokes xtask, otherwise `cargo` from `PATH`.
fn env_or_cargo() -> String {
    ferrin_provider_util::settings::env_var("CARGO").unwrap_or_else(|| "cargo".to_owned())
}

/// Maximum number of differing lines reported per crate.
const MAX_REPORTED_LINES: usize = 40;

/// Prints the lines where the committed summary and the freshly rendered one
/// disagree, so a CI log is enough to see platform-specific differences.
fn print_differences(existing: &str, rendered: &str) {
    let existing: Vec<&str> = existing.lines().collect();
    let rendered: Vec<&str> = rendered.lines().collect();
    let mut reported = 0;
    for (index, pair) in existing
        .iter()
        .map(Some)
        .chain(std::iter::repeat(None))
        .zip(rendered.iter().map(Some).chain(std::iter::repeat(None)))
        .enumerate()
        .take(existing.len().max(rendered.len()))
    {
        match pair {
            (Some(old), Some(new)) if old == new => continue,
            (old, new) => {
                if reported == MAX_REPORTED_LINES {
                    println!("  ... (more differences omitted)");
                    return;
                }
                reported += 1;
                println!(
                    "  line {}: committed {:?} / rendered {:?}",
                    index + 1,
                    old.copied().unwrap_or("<missing>"),
                    new.copied().unwrap_or("<missing>")
                );
            }
        }
    }
}

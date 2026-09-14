//! `cargo xtask check-module-size`.

use std::path::Path;

use anyhow::Context;
use anyhow::Result;

use crate::workspace::metadata_no_deps;

pub(crate) fn run(threshold: usize) -> Result<()> {
    let metadata = metadata_no_deps()?;
    let mut warnings = 0usize;
    for package in metadata.workspace_packages() {
        let root = package
            .manifest_path
            .parent()
            .context("manifest without parent directory")?
            .join("src");
        visit(root.as_std_path(), threshold, &mut warnings)?;
    }
    if warnings > 0 {
        eprintln!("{warnings} file(s) exceed {threshold} lines (warning only)");
    }
    Ok(())
}

fn visit(dir: &Path, threshold: usize, warnings: &mut usize) -> Result<()> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(());
    };
    for entry in entries {
        let path = entry?.path();
        if path.is_dir() {
            visit(&path, threshold, warnings)?;
            continue;
        }
        let is_rust = path.extension().is_some_and(|ext| ext == "rs");
        let is_test = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with("_tests.rs"));
        if !is_rust || is_test {
            continue;
        }
        let lines = std::fs::read_to_string(&path)?.lines().count();
        if lines > threshold {
            *warnings += 1;
            println!("{}: {lines} lines", path.display());
        }
    }
    Ok(())
}

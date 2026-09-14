//! `cargo xtask publish-order`.

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use anyhow::Result;
use anyhow::bail;
use cargo_metadata::DependencyKind;

use crate::workspace::is_publishable;
use crate::workspace::metadata_no_deps;

/// Kahn topological sort over publishable workspace members, using normal and
/// build dependencies only. Ties are broken by crate name for stable output.
pub(crate) fn run() -> Result<()> {
    for name in order()? {
        println!("{name}");
    }
    Ok(())
}

/// Publishable workspace crates in dependency order.
pub(crate) fn order() -> Result<Vec<String>> {
    let metadata = metadata_no_deps()?;
    let members: BTreeMap<String, &cargo_metadata::Package> = metadata
        .workspace_packages()
        .into_iter()
        .filter(|package| is_publishable(package))
        .map(|package| (package.name.to_string(), package))
        .collect();

    let mut remaining: BTreeMap<&str, BTreeSet<&str>> = members
        .iter()
        .map(|(name, package)| {
            let deps = package
                .dependencies
                .iter()
                .filter(|dependency| {
                    matches!(
                        dependency.kind,
                        DependencyKind::Normal | DependencyKind::Build
                    )
                })
                .map(|dependency| dependency.name.as_str())
                .filter(|dependency| members.contains_key(*dependency))
                .collect();
            (name.as_str(), deps)
        })
        .collect();

    let mut ordered = Vec::with_capacity(remaining.len());
    while !remaining.is_empty() {
        let ready: Vec<&str> = remaining
            .iter()
            .filter(|(_, deps)| deps.is_empty())
            .map(|(name, _)| *name)
            .collect();
        if ready.is_empty() {
            bail!(
                "dependency cycle among workspace crates: {:?}",
                remaining.keys().collect::<Vec<_>>()
            );
        }
        for name in &ready {
            ordered.push((*name).to_owned());
            remaining.remove(name);
        }
        for deps in remaining.values_mut() {
            for name in &ready {
                deps.remove(name);
            }
        }
    }
    Ok(ordered)
}

//! `cargo xtask check-versions`: resolved versions of the direct external
//! dependencies versus the latest stable release in the crates.io sparse
//! index.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use cargo_metadata::semver::Version;
use ferrin_provider_util::HttpRequest;
use ferrin_provider_util::HttpTransport;
use ferrin_provider_util::SharedTransport;
use ferrin_provider_util::default_transport;
use ferrin_provider_util::http::read_body;
use serde::Deserialize;
use tokio::task::JoinSet;
use url::Url;

const INDEX_BASE: &str = "https://index.crates.io/";
const MAX_INDEX_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Deserialize)]
struct IndexEntry {
    vers: String,
    #[serde(default)]
    yanked: bool,
}

/// Path of `name` in the sparse index (`1/a`, `2/ab`, `3/a/abc`, `ab/cd/abcd`).
fn index_path(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    match lower.len() {
        1 => format!("1/{lower}"),
        2 => format!("2/{lower}"),
        3 => format!("3/{}/{lower}", &lower[..1]),
        _ => format!("{}/{}/{lower}", &lower[..2], &lower[2..4]),
    }
}

/// Latest non-yanked, non-prerelease version listed in an index file.
fn latest_stable(index: &str) -> Option<Version> {
    index
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str::<IndexEntry>(line).ok())
        .filter(|entry| !entry.yanked)
        .filter_map(|entry| Version::parse(&entry.vers).ok())
        .filter(|version| version.pre.is_empty())
        .max()
}

async fn fetch_latest(transport: SharedTransport, name: String) -> (String, Result<Version>) {
    let result = async {
        let url = Url::parse(&format!("{INDEX_BASE}{}", index_path(&name)))
            .context("invalid index url")?;
        let response = transport
            .execute(HttpRequest::get(url))
            .await
            .map_err(|error| anyhow::anyhow!("{}", error.message))?;
        if !response.status.is_success() {
            bail!("index returned HTTP {}", response.status);
        }
        let body = read_body(&response.headers, response.body, MAX_INDEX_BYTES)
            .await
            .map_err(|error| anyhow::anyhow!("{}", error.message))?;
        let text = String::from_utf8(body.to_vec()).context("index file is not UTF-8")?;
        latest_stable(&text).context("no stable version listed")
    }
    .await;
    (name, result)
}

/// Direct external dependencies of the workspace members with their resolved
/// versions (the highest one when several are in the lock file).
fn resolved_direct_dependencies() -> Result<BTreeMap<String, Version>> {
    let metadata = crate::workspace::metadata_with_deps()?;
    let members: BTreeSet<String> = metadata
        .workspace_packages()
        .iter()
        .map(|package| package.name.to_string())
        .collect();
    let direct: BTreeSet<String> = metadata
        .workspace_packages()
        .iter()
        .flat_map(|package| package.dependencies.iter())
        .filter(|dependency| dependency.path.is_none())
        .map(|dependency| dependency.name.clone())
        .filter(|name| !members.contains(name))
        .collect();
    let mut resolved: BTreeMap<String, Version> = BTreeMap::new();
    for package in &metadata.packages {
        let name = package.name.to_string();
        if !direct.contains(&name) {
            continue;
        }
        if !package
            .source
            .as_ref()
            .is_some_and(cargo_metadata::Source::is_crates_io)
        {
            continue;
        }
        let entry = resolved
            .entry(name)
            .or_insert_with(|| package.version.clone());
        if package.version > *entry {
            *entry = package.version.clone();
        }
    }
    Ok(resolved)
}

pub(crate) fn run() -> Result<()> {
    let resolved = resolved_direct_dependencies()?;
    let transport = default_transport().map_err(|error| anyhow::anyhow!("{}", error.message))?;
    let runtime = crate::workspace::runtime()?;
    let latest = runtime.block_on(async {
        let mut tasks = JoinSet::new();
        for name in resolved.keys() {
            tasks.spawn(fetch_latest(Arc::clone(&transport), name.clone()));
        }
        let mut latest = BTreeMap::new();
        while let Some(joined) = tasks.join_next().await {
            let (name, result) = joined.context("index lookup task failed")?;
            latest.insert(name, result);
        }
        Ok::<_, anyhow::Error>(latest)
    })?;

    let mut outdated = Vec::new();
    let mut failures = Vec::new();
    for (name, current) in &resolved {
        match latest.get(name) {
            Some(Ok(newest)) if newest > current => outdated.push((name, current, newest)),
            Some(Ok(_)) => {}
            Some(Err(error)) => failures.push((name, error.to_string())),
            None => failures.push((name, "no lookup result".to_owned())),
        }
    }

    println!(
        "# Dependency versions ({})",
        chrono::Utc::now().format("%Y-%m-%d")
    );
    println!();
    println!(
        "{} direct external dependencies checked against the crates.io index.",
        resolved.len()
    );
    println!();
    if outdated.is_empty() {
        println!("All dependencies resolve to their latest stable versions.");
    } else {
        println!("| Crate | Resolved | Latest stable |");
        println!("| --- | --- | --- |");
        for (name, current, newest) in &outdated {
            println!("| `{name}` | {current} | {newest} |");
        }
    }
    if !failures.is_empty() {
        println!();
        println!("Lookups that failed:");
        for (name, error) in &failures {
            println!("- `{name}`: {error}");
        }
    }
    if !outdated.is_empty() {
        bail!("{} outdated dependencies", outdated.len());
    }
    if !failures.is_empty() {
        bail!("{} index lookups failed", failures.len());
    }
    Ok(())
}

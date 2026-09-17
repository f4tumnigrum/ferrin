//! Interactions citations derived from the Vercel AI SDK (Apache-2.0,
//! Copyright 2023 Vercel, Inc.); see the root NOTICE.

use std::collections::BTreeSet;

use ferrin_spec::Content;
use ferrin_spec::JsonValue;
use ferrin_spec::language_model::Source;

use crate::config::GoogleConfig;

fn nonempty<'a>(value: &'a JsonValue, field: &str) -> Option<&'a str> {
    value.get(field)?.as_str().filter(|value| !value.is_empty())
}

fn url(config: &GoogleConfig, value: &JsonValue, title_key: &str) -> Option<Source> {
    Some(Source::Url {
        id: config.generate_id(),
        url: nonempty(value, "url")?.to_owned(),
        title: nonempty(value, title_key).map(str::to_owned),
        provider_metadata: None,
    })
}

fn document(config: &GoogleConfig, value: &JsonValue) -> Option<Source> {
    let uri = nonempty(value, "url")
        .or_else(|| nonempty(value, "document_uri"))
        .or_else(|| nonempty(value, "file_name"))?;
    let title = nonempty(value, "title").or_else(|| nonempty(value, "file_name"));
    if uri.starts_with("https://") || uri.starts_with("http://") {
        return Some(Source::Url {
            id: config.generate_id(),
            url: uri.to_owned(),
            title: title.map(str::to_owned),
            provider_metadata: None,
        });
    }
    let filename = nonempty(value, "file_name")
        .or_else(|| uri.rsplit('/').next().filter(|part| !part.is_empty()));
    let extension = uri
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let media_type = match extension.as_str() {
        "pdf" => "application/pdf",
        "txt" => "text/plain",
        "md" | "markdown" => "text/markdown",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        _ => "application/octet-stream",
    };
    Some(Source::Document {
        id: config.generate_id(),
        media_type: media_type.into(),
        title: title.or(filename).unwrap_or(uri).to_owned(),
        filename: filename.map(str::to_owned),
        provider_metadata: None,
    })
}

pub(super) fn key(source: &Source) -> String {
    match source {
        Source::Url { url, .. } => format!("url:{url}"),
        Source::Document {
            filename, title, ..
        } => format!("doc:{}", filename.as_deref().unwrap_or(title)),
        _ => format!("{source:?}"),
    }
}

pub(super) fn extract(config: &GoogleConfig, block: &JsonValue) -> Vec<Content> {
    let mut sources = Vec::new();
    for annotation in block["annotations"].as_array().into_iter().flatten() {
        let source = match annotation["type"].as_str() {
            Some("url_citation") => url(config, annotation, "title"),
            Some("place_citation") => url(config, annotation, "name"),
            Some("file_citation") => document(config, annotation),
            _ => None,
        };
        sources.extend(source);
    }
    for result in block["result"].as_array().into_iter().flatten() {
        match block["type"].as_str() {
            Some("google_search_result") => sources.extend(url(config, result, "title")),
            Some("url_context_result") => {
                if result
                    .get("status")
                    .is_none_or(|status| status.is_null() || status == "success")
                {
                    sources.extend(url(config, result, "title"));
                }
            }
            Some("google_maps_result") => {
                for place in result["places"].as_array().into_iter().flatten() {
                    sources.extend(url(config, place, "name"));
                }
            }
            Some("file_search_result") => sources.extend(document(config, result)),
            _ => {}
        }
    }
    let mut seen = BTreeSet::new();
    sources
        .into_iter()
        .filter(|source| seen.insert(key(source)))
        .map(Content::Source)
        .collect()
}

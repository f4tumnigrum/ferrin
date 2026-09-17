//! Modality-specific aggregation of provider metadata.
//!
//! Derived from the Vercel AI SDK (Apache-2.0, Copyright 2023 Vercel, Inc.),
//! translated from TypeScript to Rust and modified; see `NOTICE`.

use ferrin_spec::JsonValue;
use ferrin_spec::ProviderMetadata;

/// Embedding metadata uses a shallow merge within each provider namespace.
pub(crate) fn accumulate_embedding_metadata(
    target: &mut Option<ProviderMetadata>,
    source: Option<&ProviderMetadata>,
) {
    let Some(source) = source else { return };
    let target = target.get_or_insert_with(ProviderMetadata::new);
    for (provider, metadata) in source {
        target
            .entry(provider.clone())
            .or_default()
            .extend(metadata.clone());
    }
}

/// Video lists concatenate; other metadata fields use the latest value.
#[cfg(feature = "video")]
pub(crate) fn merge_video_metadata(target: &mut ProviderMetadata, source: &ProviderMetadata) {
    for (provider, metadata) in source {
        let entry = target.entry(provider.clone()).or_default();
        for (key, value) in metadata {
            match (key.as_str(), entry.get_mut(key), value) {
                ("videos", Some(JsonValue::Array(existing)), JsonValue::Array(incoming)) => {
                    existing.extend(incoming.iter().cloned());
                }
                _ => {
                    entry.insert(key.clone(), value.clone());
                }
            }
        }
    }
}

/// Image lists concatenate; gateway decimal costs sum without floating-point loss.
pub(crate) fn merge_image_metadata(target: &mut ProviderMetadata, source: &ProviderMetadata) {
    const COST_KEYS: [&str; 7] = [
        "cost",
        "gatewayCost",
        "inferenceCost",
        "inputInferenceCost",
        "marketCost",
        "outputInferenceCost",
        "surchargeCost",
    ];
    for (provider, metadata) in source {
        let entry = target.entry(provider.clone()).or_default();
        if provider == "gateway" {
            let sums: Vec<_> = COST_KEYS
                .iter()
                .filter_map(|key| {
                    Some((
                        (*key).to_owned(),
                        JsonValue::String(add_decimal_strings(
                            entry.get(*key)?.as_str()?,
                            metadata.get(*key)?.as_str()?,
                        )?),
                    ))
                })
                .collect();
            entry.extend(metadata.clone());
            entry.extend(sums);
            if entry
                .get("images")
                .is_some_and(|value| value.as_array().is_some_and(Vec::is_empty))
            {
                entry.remove("images");
            }
        } else {
            let images = entry
                .entry("images".to_owned())
                .or_insert_with(|| JsonValue::Array(Vec::new()));
            if let (Some(existing), Some(incoming)) = (
                images.as_array_mut(),
                metadata.get("images").and_then(JsonValue::as_array),
            ) {
                existing.extend(incoming.iter().cloned());
            }
        }
    }
}

fn decimal_parts(value: &str) -> Option<(&str, &str)> {
    let (integer, fraction) = value.split_once('.').unwrap_or((value, ""));
    if integer.is_empty()
        || !integer.bytes().all(|byte| byte.is_ascii_digit())
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
        || (value.contains('.') && fraction.is_empty())
    {
        return None;
    }
    Some((integer, fraction))
}

fn add_decimal_strings(left: &str, right: &str) -> Option<String> {
    let (left_integer, left_fraction) = decimal_parts(left)?;
    let (right_integer, right_fraction) = decimal_parts(right)?;
    let precision = left_fraction.len().max(right_fraction.len());
    let digits = |integer: &str, fraction: &str| {
        integer
            .bytes()
            .chain(fraction.bytes())
            .chain(std::iter::repeat_n(b'0', precision - fraction.len()))
            .collect::<Vec<_>>()
    };
    let left = digits(left_integer, left_fraction);
    let right = digits(right_integer, right_fraction);
    let mut result = Vec::with_capacity(left.len().max(right.len()) + 1);
    let mut carry = 0;
    for index in 0..left.len().max(right.len()) {
        let digit = |value: &[u8]| {
            value
                .len()
                .checked_sub(index + 1)
                .and_then(|position| value.get(position))
                .map_or(0, |digit| digit - b'0')
        };
        let sum = digit(&left) + digit(&right) + carry;
        result.push(char::from(b'0' + sum % 10));
        carry = sum / 10;
    }
    if carry != 0 {
        result.push(char::from(b'0' + carry));
    }
    while result.len() > precision + 1 && result.last() == Some(&'0') {
        result.pop();
    }
    result.reverse();
    if precision > 0 {
        result.insert(result.len() - precision, '.');
    }
    let mut result: String = result.into_iter().collect();
    if precision > 0 {
        while result.ends_with('0') {
            result.pop();
        }
        if result.ends_with('.') {
            result.pop();
        }
    }
    Some(result)
}

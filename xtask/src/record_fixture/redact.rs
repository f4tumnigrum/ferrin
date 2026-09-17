//! Reproducible response redaction before fixtures are written.

use anyhow::Context;
use anyhow::Result;
use ferrin_provider_util::sse::SseDecoder;
use serde_json::Value;

pub(crate) fn json(value: &mut Value, pointers: &[String]) -> bool {
    let mut changed = false;
    for pointer in pointers {
        if let Some(field) = value.pointer_mut(pointer)
            && !field.is_null()
        {
            *field = Value::String("[REDACTED]".to_owned());
            changed = true;
        }
    }
    changed
}

pub(crate) fn sse_event(event: &str, pointers: &[String]) -> Result<String> {
    if pointers.is_empty() {
        return Ok(event.to_owned());
    }
    let events = SseDecoder::new().feed(format!("{event}\n\n").as_bytes())?;
    let Some(decoded) = events.first() else {
        return Ok(event.to_owned());
    };
    if decoded.data == "[DONE]" {
        return Ok(event.to_owned());
    }
    let mut value: Value =
        serde_json::from_str(&decoded.data).context("cannot redact non-JSON SSE data")?;
    if !json(&mut value, pointers) {
        return Ok(event.to_owned());
    }
    let replacement = format!("data: {}", serde_json::to_string(&value)?);
    let mut lines = Vec::new();
    let mut replaced = false;
    // Preserve event names, IDs, retry hints and comments around multiline data.
    for line in event.lines() {
        if line == "data" || line.starts_with("data:") {
            if !replaced {
                lines.push(replacement.as_str());
                replaced = true;
            }
        } else {
            lines.push(line);
        }
    }
    Ok(lines.join("\n"))
}

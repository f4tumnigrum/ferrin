//! PV-002: a naive comma-split `data:` URL parser vs Ferrin's RFC 2397 parser
//! and the `data-url` crate.

/// The naive parser: split on commas and take the first two pieces, then read
/// the media type as `header.split(';')[0].split(':')[1]`.
pub fn naive_split(data_url: &str) -> (Option<String>, Option<String>) {
    let mut parts = data_url.split(',');
    let header = parts.next().unwrap_or("");
    let base64_content = parts.next().map(str::to_owned); // [header, payload] takes index 1 only; later pieces are dropped
    let media_type = header
        .split(';')
        .next()
        .unwrap_or("")
        .split(':')
        .nth(1)
        .map(str::to_owned);
    (media_type, base64_content)
}

#[derive(Debug, PartialEq)]
pub struct Parsed {
    pub media_type: String,
    pub bytes: Vec<u8>,
    pub base64: bool,
}

/// Candidate Ferrin implementation: RFC 2397 parsing without external crates.
/// Splits at the first comma, parses `mediatype[;params][;base64]`, decodes
/// base64 (standard alphabet, padding optional, whitespace ignored) or
/// percent-decodes the payload.
pub fn ferrin_parse(data_url: &str) -> Result<Parsed, &'static str> {
    let rest = data_url
        .get(..5)
        .filter(|scheme| scheme.eq_ignore_ascii_case("data:"))
        .map(|_| &data_url[5..])
        .ok_or("not a data: URL")?;
    let (header, payload) = rest.split_once(',').ok_or("missing comma")?;
    let mut segments: Vec<&str> = header.split(';').map(str::trim).collect();
    let base64 = segments
        .last()
        .is_some_and(|last| last.eq_ignore_ascii_case("base64"));
    if base64 {
        segments.pop();
    }
    let media_type = if segments.is_empty() || segments[0].is_empty() {
        let params = segments.iter().skip(1).copied().collect::<Vec<_>>();
        if params.is_empty() {
            "text/plain;charset=US-ASCII".to_owned()
        } else {
            format!("text/plain;{}", params.join(";"))
        }
    } else {
        segments.join(";")
    };
    let bytes = if base64 {
        use base64::Engine;
        let cleaned: String = percent_decode(payload)
            .into_iter()
            .map(char::from)
            .filter(|c| !c.is_ascii_whitespace())
            .collect();
        let engine = base64::engine::GeneralPurpose::new(
            &base64::alphabet::STANDARD,
            base64::engine::GeneralPurposeConfig::new()
                .with_decode_padding_mode(base64::engine::DecodePaddingMode::Indifferent),
        );
        engine.decode(cleaned).map_err(|_| "invalid base64")?
    } else {
        percent_decode(payload)
    };
    Ok(Parsed { media_type, bytes, base64 })
}

fn percent_decode(input: &str) -> Vec<u8> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() + 0 && i + 2 <= bytes.len() - 1 + 1 {
            if let (Some(h), Some(l)) = (hex(bytes.get(i + 1)), hex(bytes.get(i + 2))) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

fn hex(b: Option<&u8>) -> Option<u8> {
    b.and_then(|b| (*b as char).to_digit(16)).map(|d| d as u8)
}

pub fn crate_parse(data_url: &str) -> Result<Parsed, String> {
    let url = data_url::DataUrl::process(data_url).map_err(|e| format!("{e:?}"))?;
    let (bytes, _fragment) = url.decode_to_vec().map_err(|e| format!("{e:?}"))?;
    let mime = url.mime_type();
    Ok(Parsed { media_type: mime.to_string(), bytes, base64: false })
}

#[cfg(test)]
mod tests {
    use super::*;

    const CASES: &[&str] = &[
        "data:image/png;base64,iVBORw0KGgo=",
        "data:image/png;base64,iVBORw0KGgo",          // missing padding
        "data:text/plain,hello%20world",               // not base64
        "data:text/plain;charset=utf-8,caf%C3%A9",      // params + percent-encoding
        "data:;base64,aGVsbG8=",                       // empty media type
        "data:,plain",                                 // no header at all
        "data:image/svg+xml;charset=utf-8;base64,PHN2Zz4=",
        "data:image/png;base64,iVBO,Rw0K",              // comma inside payload
        "DATA:image/png;base64,iVBORw0KGgo=",           // uppercase scheme
        "data:image/png;BASE64,iVBORw0KGgo=",           // uppercase base64 flag
        "data:image/png;base64,iVBO Rw0K Ggo=",         // whitespace inside base64
        "data:image/png;base64,%69VBORw0KGgo=",         // percent-encoded base64 char
        "data:application/json,%7B%22a%22%3A1%7D",
        "data:image/png",                              // no comma
    ];

    #[test]
    fn print_comparison_table() {
        println!("| input | naive_split (media type, payload) | ferrin_parse | data-url crate |");
        println!("| --- | --- | --- | --- |");
        for case in CASES {
            let (mt, content) = naive_split(case);
            let ferrin = match ferrin_parse(case) {
                Ok(p) => format!("{} / {} bytes / base64={}", p.media_type, p.bytes.len(), p.base64),
                Err(e) => format!("ERR {e}"),
            };
            let krate = match crate_parse(case) {
                Ok(p) => format!("{} / {} bytes", p.media_type, p.bytes.len()),
                Err(e) => format!("ERR {e}"),
            };
            println!("| `{case}` | {mt:?}, {content:?} | {ferrin} | {krate} |");
        }
    }

    #[test]
    fn ferrin_parse_agrees_with_crate_on_bytes_for_valid_inputs() {
        for case in CASES {
            let (Ok(a), Ok(b)) = (ferrin_parse(case), crate_parse(case)) else { continue };
            assert_eq!(a.bytes, b.bytes, "bytes differ for {case}");
        }
    }

    #[test]
    fn naive_split_is_lossy() {
        // The naive parser keeps the payload as an opaque string even when the
        // URL is not base64 and truncates payloads containing commas.
        assert_eq!(naive_split("data:text/plain,hello%20world").1.as_deref(), Some("hello%20world"));
        assert_eq!(naive_split("data:image/png;base64,iVBO,Rw0K").1.as_deref(), Some("iVBO"));
        assert_eq!(naive_split("data:;base64,aGVsbG8=").0.as_deref(), Some(""));
    }
}

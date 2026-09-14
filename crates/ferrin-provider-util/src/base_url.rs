//! Base URL validation.

use ferrin_spec::error::InvalidArgumentError;
use url::Url;

/// Parses a base URL: non-empty, absolute, trailing slash removed.
///
/// # Errors
///
/// Returns [`InvalidArgumentError`] (argument `base_url`) for empty or
/// unparsable input.
pub fn parse_base_url(base_url: &str) -> Result<Url, InvalidArgumentError> {
    if base_url.trim().is_empty() {
        return Err(InvalidArgumentError::new(
            "base_url",
            "base_url must be a non-empty string.",
        ));
    }
    let url = Url::parse(base_url.trim()).map_err(|error| {
        InvalidArgumentError::new("base_url", format!("base_url is not a valid URL: {error}"))
    })?;
    Ok(without_trailing_slash(url))
}

/// Removes one trailing slash from the path.
#[must_use]
pub fn without_trailing_slash(mut url: Url) -> Url {
    let path = url.path();
    if path.len() > 1 && path.ends_with('/') {
        let trimmed = path[..path.len() - 1].to_owned();
        url.set_path(&trimmed);
    }
    url
}

/// Joins `path` onto a base URL whose path is preserved (`/v1` + `/chat` →
/// `/v1/chat`).
#[must_use]
pub fn join_path(base: &Url, path: &str) -> Url {
    let mut url = base.clone();
    let base_path = base.path().trim_end_matches('/');
    let joined = format!("{base_path}/{}", path.trim_start_matches('/'));
    url.set_path(&joined);
    url.set_query(None);
    url
}

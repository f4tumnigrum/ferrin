//! Optional validation and normalization of typed agent options.

use std::sync::Arc;

use ferrin_schema::Schema;
use serde::Serialize;

use crate::error::Error;

pub(super) type CallOptionsValidator<Opt> = Arc<dyn Fn(Opt) -> Result<Opt, Error> + Send + Sync>;

pub(super) fn validator<Opt: Serialize + 'static>(
    schema: Schema<Opt>,
) -> CallOptionsValidator<Opt> {
    Arc::new(move |options| {
        let value = serde_json::to_value(options)
            .map_err(|_| Error::invalid_argument("options", "cannot serialize call options"))?;
        schema.validate(value).map_err(|_| {
            Error::invalid_argument("options", "call options failed schema validation")
        })
    })
}

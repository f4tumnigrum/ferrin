use ferrin_core::Error;
use static_assertions::const_assert;

const_assert!(size_of::<Error>() <= 128);
const_assert!(size_of::<Result<(), Error>>() <= 128);

#[test]
fn error_kind_names_are_kebab_case() {
    assert_eq!(Error::Cancelled.kind().as_str(), "cancelled");
    assert_eq!(
        Error::invalid_stream_part("x").kind().as_str(),
        "invalid-input"
    );
    assert_eq!(Error::NoOutputGenerated.kind().as_str(), "output");
}

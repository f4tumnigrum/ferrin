use pretty_assertions::assert_eq;
use serde_json::json;

use ferrin_spec::AspectRatio;
use ferrin_spec::ImageSize;

#[test]
fn size_and_ratio_parse_and_serialize() {
    let size: ImageSize = "1024x768".parse().unwrap();
    assert_eq!(size, ImageSize::new(1024, 768));
    assert_eq!(serde_json::to_value(size).unwrap(), json!("1024x768"));
    let parsed: ImageSize = serde_json::from_value(json!("512x512")).unwrap();
    assert_eq!(parsed.to_string(), "512x512");

    let ratio: AspectRatio = "16:9".parse().unwrap();
    assert_eq!(ratio, AspectRatio::new(16, 9));
    assert_eq!(serde_json::to_value(ratio).unwrap(), json!("16:9"));

    for bad in ["1024", "0x10", "ax10", "16/9", ""] {
        assert!(bad.parse::<ImageSize>().is_err(), "{bad:?}");
    }
    assert!(serde_json::from_value::<AspectRatio>(json!("16x9")).is_err());
}

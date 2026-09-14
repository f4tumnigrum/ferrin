use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use ferrin_provider_util::media_type::detect_media_type;
use ferrin_provider_util::media_type::detect_media_type_base64;
use ferrin_provider_util::media_type::detect_media_type_for;
use ferrin_provider_util::media_type::media_type_to_extension;
use ferrin_provider_util::media_type::resolve_full_media_type;
use ferrin_spec::MediaType;
use pretty_assertions::assert_eq;

fn media(value: &str) -> Option<MediaType> {
    Some(MediaType::new(value))
}

#[test]
fn detects_common_signatures() {
    assert_eq!(detect_media_type(b"\x89PNG\r\n\x1a\n"), media("image/png"));
    assert_eq!(detect_media_type(b"\xff\xd8\xff\xe0"), media("image/jpeg"));
    assert_eq!(detect_media_type(b"GIF89a"), media("image/gif"));
    assert_eq!(
        detect_media_type(b"RIFF\x00\x00\x00\x00WEBPVP8 "),
        media("image/webp")
    );
    assert_eq!(detect_media_type(b"%PDF-1.7"), media("application/pdf"));
    assert_eq!(
        detect_media_type(b"RIFF\x00\x00\x00\x00WAVEfmt "),
        media("audio/wav")
    );
    assert_eq!(detect_media_type(b"OggS\x00\x02"), media("audio/ogg"));
    assert_eq!(detect_media_type(b"\xff\xfb\x90\x00"), media("audio/mpeg"));
    assert_eq!(detect_media_type(b"\x1a\x45\xdf\xa3"), media("audio/webm"));
    assert_eq!(
        detect_media_type(b"\x00\x00\x00\x18ftypisom"),
        media("video/mp4")
    );
    assert_eq!(detect_media_type(b"hello"), None);
}

#[test]
fn top_level_selects_table() {
    let mp4 = b"\x00\x00\x00\x18ftypM4A ";
    assert_eq!(detect_media_type_for(mp4, "audio"), media("audio/mp4"));
    assert_eq!(detect_media_type_for(mp4, "video"), media("video/mp4"));
    assert_eq!(detect_media_type_for(mp4, "image"), None);
    assert_eq!(
        detect_media_type_for(b"\x1a\x45\xdf\xa3", "video"),
        media("video/webm")
    );
}

#[test]
fn strips_id3_tags_before_sniffing() {
    let mut bytes = b"ID3\x04\x00\x00\x00\x00\x00\x0a".to_vec();
    bytes.extend_from_slice(&[0u8; 10]);
    bytes.extend_from_slice(b"\xff\xfb\x90\x00");
    assert_eq!(detect_media_type(&bytes), media("audio/mpeg"));
    let encoded = BASE64_STANDARD.encode(&bytes);
    assert_eq!(
        detect_media_type_base64(&encoded, Some("audio")),
        media("audio/mpeg")
    );
}

#[test]
fn base64_prefix_detection() {
    let png = BASE64_STANDARD.encode(b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x01");
    assert_eq!(detect_media_type_base64(&png, None), media("image/png"));
    assert_eq!(detect_media_type_base64("aGVsbG8=", None), None);
}

#[test]
fn extension_and_resolution() {
    assert_eq!(
        media_type_to_extension(&MediaType::new("audio/mpeg")),
        "mp3"
    );
    assert_eq!(
        media_type_to_extension(&MediaType::new("audio/x-wav")),
        "wav"
    );
    assert_eq!(media_type_to_extension(&MediaType::new("audio/mp4")), "m4a");
    assert_eq!(media_type_to_extension(&MediaType::new("image/PNG")), "png");

    let full = resolve_full_media_type(&MediaType::new("image/png"), None).unwrap();
    assert_eq!(full, MediaType::new("image/png"));
    let sniffed =
        resolve_full_media_type(&MediaType::new("image/*"), Some(b"\x89PNG\r\n\x1a\n")).unwrap();
    assert_eq!(sniffed, MediaType::new("image/png"));
    let error = resolve_full_media_type(&MediaType::new("image"), None).unwrap_err();
    assert!(error.to_string().contains("not passed as inline bytes"));
    let error = resolve_full_media_type(&MediaType::new("image/*"), Some(b"nope")).unwrap_err();
    assert!(error.to_string().contains("could not be auto-detected"));
}

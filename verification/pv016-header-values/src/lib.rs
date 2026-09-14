//! PV-016: how `http` 1.5 `HeaderValue` treats non-ASCII values.
#[cfg(test)]
mod tests {
    use http::HeaderValue;

    #[test]
    fn non_ascii_behaviour() {
        let from_str = HeaderValue::from_str("café");
        let from_bytes = HeaderValue::from_bytes("café".as_bytes());
        let ctrl = HeaderValue::from_bytes(b"a\nb");
        let del = HeaderValue::from_bytes(b"a\x7fb");
        let tab = HeaderValue::from_bytes(b"a\tb");
        println!("from_str(caf\u{e9}) = {:?}", from_str.as_ref().map(|_| "ok"));
        println!("from_bytes(caf\u{e9} utf8) = {:?}", from_bytes.as_ref().map(|_| "ok"));
        println!("from_bytes(LF) = {:?}, from_bytes(DEL) = {:?}, from_bytes(TAB) = {:?}", ctrl.is_ok(), del.is_ok(), tab.is_ok());
        let value = from_bytes.unwrap();
        println!("to_str() on utf8 value = {:?}; as_bytes len = {}", value.to_str().map(|_| "ok"), value.as_bytes().len());
        // http 1.5: from_str accepts obs-text bytes (0x80..=0xFF), so UTF-8 passes; only to_str() rejects it.
        assert!(from_str.is_ok());
        assert!(value.to_str().is_err());
        assert!(ctrl.is_err() && del.is_err() && tab.is_ok());
    }
}

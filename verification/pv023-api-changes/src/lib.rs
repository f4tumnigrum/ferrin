//! PV-023: API shapes in base64 0.23, rand 0.10 and tokio-tungstenite 0.30.

use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use base64::prelude::BASE64_URL_SAFE_NO_PAD;
use rand::RngExt;
use rand::distr::Alphanumeric;

pub fn b64(data: &[u8]) -> (String, String) {
    (BASE64_STANDARD.encode(data), BASE64_URL_SAFE_NO_PAD.encode(data))
}

pub fn random_id(prefix: &str, len: usize) -> String {
    let suffix: String = rand::rng().sample_iter(Alphanumeric).take(len).map(char::from).collect();
    format!("{prefix}-{suffix}")
}

pub fn ws_connector_type_exists() -> &'static str {
    // Named types from tokio-tungstenite 0.30 with the rustls feature.
    fn _assert<T>() {}
    _assert::<tokio_tungstenite::Connector>();
    _assert::<tokio_tungstenite::tungstenite::Message>();
    std::any::type_name::<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apis() {
        let (std, url) = b64(b"hello?>");
        assert_eq!(std, "aGVsbG8/Pg==");
        assert_eq!(url, "aGVsbG8_Pg");
        let id = random_id("call", 16);
        assert_eq!(id.len(), "call-".len() + 16);
        println!("base64 ok: {std} {url}; rand id: {id}; ws stream type: {}", ws_connector_type_exists());
    }
}

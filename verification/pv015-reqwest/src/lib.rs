//! PV-015: reqwest 0.13.5 API surface needed by the secure fetch path, plus the
//! cost of building one client per download target.

use std::net::SocketAddr;
use std::time::Duration;
use std::time::Instant;

/// Builds the pinned client exactly as `secure_url::fetch` would: DNS pinned to
/// pre-resolved addresses, no redirects, timeouts.
pub fn pinned_client(host: &str, addrs: &[SocketAddr]) -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder()
        .resolve_to_addrs(host, addrs)
        .redirect(reqwest::redirect::Policy::none())
        .https_only(true)
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(60))
        .http1_max_headers(100)
        .user_agent("ferrin-pv015")
        .build()
}

pub fn build_many(n: usize) -> (Duration, usize) {
    let addrs = [SocketAddr::from(([93, 184, 216, 34], 443))];
    let start = Instant::now();
    let clients: Vec<reqwest::Client> = (0..n).map(|_| pinned_client("example.com", &addrs).expect("client")).collect();
    (start.elapsed(), clients.len())
}

pub fn error_is_dns_exists(err: &reqwest::Error) -> bool {
    err.is_dns() || err.is_connect() || err.is_timeout()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn api_surface_compiles_and_client_build_cost() {
        let (elapsed, n) = build_many(100);
        println!("built {n} pinned clients in {elapsed:?} ({:?} each)", elapsed / n as u32);
        // A pinned client for an unroutable address must fail fast on connect, not resolve DNS.
        let client = pinned_client("example.invalid", &[SocketAddr::from(([192, 0, 2, 1], 443))]).unwrap();
        let client = client;
        let started = Instant::now();
        let err = tokio::time::timeout(
            Duration::from_secs(3),
            client.get("https://example.invalid/").timeout(Duration::from_millis(500)).send(),
        )
        .await
        .expect("bounded")
        .expect_err("must not connect");
        println!("pinned request failed as expected after {:?}: dns={} connect={} timeout={}", started.elapsed(), err.is_dns(), err.is_connect(), err.is_timeout());
        assert!(error_is_dns_exists(&err));
        assert!(!err.is_dns(), "pinned resolution must not perform DNS");
    }
}

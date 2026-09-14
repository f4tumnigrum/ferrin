//! Fixture files and the local HTTP server that replays them.
//!
//! Fixture layout (per provider crate, `tests/fixtures/`):
//!
//! - `<case>.response.json`: a complete response body.
//! - `<case>.chunks.txt`: a streamed response, one server-sent event per
//!   line, `\n` and `\\` escaped.
//! - `<case>.meta.json`: optional `{ "status": 200, "headers": { .. } }`.
//!
//! [`FixtureServer`] serves both kinds from one address so a provider under
//! test needs a single base URL.

mod files;
mod server;

pub use files::Fixture;
pub use files::FixtureBody;
pub use files::decode_events_file;
pub use files::encode_events_file;
pub use server::FixtureServer;
pub use server::ReceivedRequest;

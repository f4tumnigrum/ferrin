//! Ferrin testing support.
//!
//! Mock models, stream simulation, the fixture server used by provider
//! tests, specification contract checks and a recording HTTP transport.
//! Intended for tests of applications and provider crates; not for
//! production use.
//!
//! Design: `docs/03-engineering/04-testing.md`.

pub mod contract;
pub mod fixture;
pub mod ids;
pub mod mock;
pub mod stream;
pub mod transport;

pub use contract::ContractViolation;
pub use contract::StreamContractChecker;
pub use fixture::Fixture;
pub use fixture::FixtureBody;
pub use fixture::FixtureServer;
pub use fixture::ReceivedRequest;
pub use ids::SequentialIdGenerator;
pub use mock::MockCallKind;
pub use mock::MockLanguageModel;
pub use mock::MockLanguageModelBuilder;
pub use mock::RecordedCall;
pub use mock::api_call_error;
pub use stream::SimulatedStream;
pub use stream::simulate_stream;
pub use stream::text_parts;
pub use transport::HeaderFilter;
pub use transport::RecordedRequest;
pub use transport::RecordedResponse;
pub use transport::RecordingTransport;
pub use transport::redact_secrets;

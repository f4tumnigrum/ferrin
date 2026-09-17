mod batch;
mod chat;
mod common;
mod completion;
mod embedding;
mod error;
mod files;
mod image;
mod live_responses;
mod provider;
mod realtime;
#[cfg(feature = "realtime")]
mod realtime_ws;
mod reference_resources;
mod reference_tool_options;
mod reference_usage;
mod responses_advanced;
mod responses_generate;
mod responses_parallel;
mod responses_recorded;
mod responses_request;
mod responses_stream;
mod skills;
mod speech;
mod stream_eof;
mod strict_schema;
mod tools;
mod transcription;

mod reference_tool_schemas;

mod reference_upload;

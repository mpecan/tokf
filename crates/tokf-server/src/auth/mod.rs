pub mod github;
// Intentionally public: integration test binaries import `NoOpGitHubClient` via
// `tokf_server::auth::mock`. A feature gate was considered but adds CI complexity
// for no real benefit — the mock types are harmless in production builds.
pub mod mock;
pub mod service_token;
pub mod token;

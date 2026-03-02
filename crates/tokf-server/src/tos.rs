/// Current Terms of Service version. Bump this when the terms change;
/// users will be prompted to re-accept on their next `tokf auth login`.
pub const CURRENT_TOS_VERSION: i32 = 1;

/// The full Terms of Service text (Markdown), embedded at compile time.
pub const TOS_CONTENT_MD: &str = include_str!("tos/content.md");

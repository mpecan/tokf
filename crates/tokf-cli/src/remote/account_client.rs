use super::http::Client;

/// Delete the authenticated user's account on the server.
///
/// The server anonymizes the user profile and cascade-deletes auth tokens,
/// machines, usage events, sync cursors, and `ToS` acceptance records.
/// Published filters are preserved with the account converted to unclaimed.
///
/// # Errors
///
/// Returns an error on network failure or non-2xx status.
pub fn delete_account(client: &Client) -> anyhow::Result<()> {
    // client.delete() already validates 2xx via require_success()
    client.delete("/api/account")?;
    Ok(())
}

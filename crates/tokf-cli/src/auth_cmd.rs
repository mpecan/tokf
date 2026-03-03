use std::io::{BufRead, Write};
use std::thread;
use std::time::{Duration, Instant};

use tokf::auth::{client, credentials};
use tokf::remote::{account_client, http::Client, machine, tos_client};

const MAX_NETWORK_RETRIES: u32 = 3;

pub fn cmd_auth_login() -> anyhow::Result<i32> {
    let base_url = client::server_url();

    // Check if already logged in — may need ToS re-acceptance
    if let Some(auth) = credentials::load() {
        return handle_existing_login(&auth, &base_url);
    }

    if !client::is_secure_url(&base_url) {
        eprintln!(
            "[tokf] WARNING: server URL uses insecure HTTP — credentials will be sent unencrypted"
        );
    }

    // Fetch current ToS version (unauthenticated)
    let tos_version = prompt_tos_acceptance(&base_url)?;

    let http_client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .connect_timeout(Duration::from_secs(5))
        .user_agent(format!("tokf-cli/{}", env!("CARGO_PKG_VERSION")))
        .build()?;

    let device_resp = client::initiate_device_flow(&http_client, &base_url)?;

    let expires_min = device_resp.expires_in.clamp(0, 1800) / 60;
    eprintln!(
        "[tokf] Your one-time code: {} (expires in {expires_min}m)",
        device_resp.user_code
    );

    // Prefer verification_uri_complete (pre-fills the code) per RFC 8628
    let browser_uri = device_resp
        .verification_uri_complete
        .as_deref()
        .unwrap_or(&device_resp.verification_uri);

    // Validate URI scheme before passing to OS
    if client::is_safe_browser_uri(browser_uri) {
        if open::that(browser_uri).is_ok() {
            eprintln!("[tokf] Opening {browser_uri} in your browser...");
        } else {
            eprintln!("[tokf] Open this URL in your browser: {browser_uri}");
        }
    } else {
        eprintln!(
            "[tokf] Open this URL in your browser: {}",
            device_resp.verification_uri
        );
    }

    eprintln!("[tokf] Waiting for authorization (press Ctrl+C to cancel)...");
    poll_for_token(&http_client, &base_url, &device_resp, tos_version)
}

/// If already logged in, check whether `ToS` re-acceptance is needed.
fn handle_existing_login(auth: &credentials::LoadedAuth, base_url: &str) -> anyhow::Result<i32> {
    // Try to fetch current ToS version from server
    let Ok(client) = Client::unauthenticated(base_url) else {
        eprintln!(
            "[tokf] Already logged in as {}. Run `tokf auth logout` first.",
            auth.username
        );
        return Ok(0);
    };

    // Server unreachable or old server without ToS — just report logged in
    let Ok(tos_info) = tos_client::fetch_tos_info(&client) else {
        eprintln!(
            "[tokf] Already logged in as {}. Run `tokf auth logout` first.",
            auth.username
        );
        return Ok(0);
    };

    // Check if local version is current
    if auth
        .tos_accepted_version
        .is_some_and(|v| v >= tos_info.version)
    {
        eprintln!(
            "[tokf] Already logged in as {}. Run `tokf auth logout` first.",
            auth.username
        );
        return Ok(0);
    }

    // ToS re-acceptance needed
    eprintln!(
        "[tokf] The Terms of Service have been updated (v{}).",
        tos_info.version
    );
    print_tos_summary(&tos_info.url);

    if !confirm_tos(tos_info.version)? {
        eprintln!(
            "[tokf] Terms declined. You remain logged in but some features may require acceptance."
        );
        return Ok(1);
    }

    // Record acceptance on server
    let authed_client = Client::authed()?;
    tos_client::accept_tos(&authed_client, tos_info.version)?;
    credentials::save_tos_accepted_version(tos_info.version)?;
    eprintln!("[tokf] Terms of Service v{} accepted.", tos_info.version);
    Ok(0)
}

/// Fetch `ToS` info and prompt the user to accept before proceeding with login.
///
/// Returns the accepted version, or an error if declined.
fn prompt_tos_acceptance(base_url: &str) -> anyhow::Result<Option<i64>> {
    let Ok(client) = Client::unauthenticated(base_url) else {
        return Ok(None); // Can't reach server — proceed without ToS
    };

    let Ok(tos_info) = tos_client::fetch_tos_info(&client) else {
        return Ok(None); // Old server without ToS endpoint
    };

    eprintln!("[tokf] Before logging in, please review our Terms of Service.");
    print_tos_summary(&tos_info.url);

    if !confirm_tos(tos_info.version)? {
        anyhow::bail!("Terms of Service declined — login cancelled");
    }

    Ok(Some(tos_info.version))
}

fn print_tos_summary(terms_url: &str) {
    eprintln!("[tokf] Summary: tokf collects your GitHub profile, machine IDs, and");
    eprintln!("[tokf]          aggregate token-count statistics. We do not collect");
    eprintln!("[tokf]          command content or output. No data is sold or shared.");
    eprintln!("[tokf]          No guarantees are provided.");
    eprintln!("[tokf] Full terms: {terms_url}");
}

fn confirm_tos(version: i64) -> anyhow::Result<bool> {
    eprint!("[tokf] Accept Terms of Service (v{version})? [y/N]: ");
    std::io::stderr().flush()?;

    let mut input = String::new();
    std::io::stdin().lock().read_line(&mut input)?;
    Ok(input.trim().eq_ignore_ascii_case("y") || input.trim().eq_ignore_ascii_case("yes"))
}

fn poll_for_token(
    http_client: &reqwest::blocking::Client,
    base_url: &str,
    device_resp: &client::DeviceFlowResponse,
    tos_version: Option<i64>,
) -> anyhow::Result<i32> {
    let mut interval = device_resp.interval.clamp(1, 60);
    let expires_in = device_resp.expires_in.clamp(0, 1800);
    let max_attempts = expires_in / interval;
    let mut consecutive_errors: u32 = 0;
    let start = Instant::now();
    let mut last_progress = 0u64;

    for _ in 0..max_attempts {
        thread::sleep(Duration::from_secs(interval.unsigned_abs()));

        match client::poll_token(http_client, base_url, &device_resp.device_code, tos_version) {
            Ok(client::PollResult::Success(token_resp)) => {
                credentials::save(
                    &token_resp.access_token,
                    &token_resp.user.username,
                    base_url,
                    token_resp.expires_in,
                )?;
                // Save the accepted ToS version locally
                if let Some(v) = tos_version {
                    credentials::save_tos_accepted_version(v)?;
                }
                eprintln!();
                eprintln!("[tokf] Logged in as {}", token_resp.user.username);

                if let Some(auth) = credentials::load() {
                    run_onboarding(&auth);
                }

                return Ok(0);
            }
            Ok(client::PollResult::Pending { .. }) => {
                consecutive_errors = 0;
                print_progress(&start, expires_in, &mut last_progress);
            }
            Ok(client::PollResult::SlowDown {
                interval: new_interval,
            }) => {
                consecutive_errors = 0;
                interval = new_interval.clamp(1, 60);
                print_progress(&start, expires_in, &mut last_progress);
            }
            Ok(client::PollResult::Failed(msg)) => {
                eprintln!();
                if msg.contains("denied") {
                    anyhow::bail!("authorization was denied");
                }
                anyhow::bail!("{msg}");
            }
            Err(e) => {
                consecutive_errors += 1;
                if consecutive_errors >= MAX_NETWORK_RETRIES {
                    eprintln!();
                    return Err(e);
                }
                eprint!("!");
            }
        }
    }

    eprintln!();
    anyhow::bail!("authorization timed out — run `tokf auth login` again");
}

/// Print a dot normally; every 30s print a time-remaining update instead.
fn print_progress(start: &Instant, expires_in: i64, last_progress_secs: &mut u64) {
    let elapsed = start.elapsed().as_secs();
    let remaining = expires_in.unsigned_abs().saturating_sub(elapsed);
    // Every 30 seconds, print a time-aware update
    if elapsed / 30 > *last_progress_secs / 30 {
        *last_progress_secs = elapsed;
        let remaining_min = remaining / 60;
        let remaining_sec = remaining % 60;
        eprint!(" ({remaining_min}m{remaining_sec:02}s left)");
    } else {
        eprint!(".");
    }
}

/// Post-login onboarding: offer machine registration and usage stats opt-in.
fn run_onboarding(auth: &credentials::LoadedAuth) {
    let machine_available = prompt_machine_registration(auth);

    if machine_available {
        prompt_usage_stats();
    }
}

/// Ask the user whether to register this machine. Returns `true` if a machine
/// is registered (either newly or already was).
fn prompt_machine_registration(auth: &credentials::LoadedAuth) -> bool {
    if machine::load().is_some() {
        return true; // already registered
    }

    eprintln!();
    eprintln!("[tokf] Would you like to register this machine for remote sync?");
    eprintln!("[tokf] This generates a machine ID and hostname, sent to the tokf server");
    eprintln!("[tokf] so your usage statistics can be associated with this device.");
    eprint!("[tokf] Register this machine? [y/N]: ");
    let _ = std::io::stderr().flush();

    let mut input = String::new();
    if std::io::stdin().lock().read_line(&mut input).is_err() {
        return false;
    }
    if !input.trim().eq_ignore_ascii_case("y") && !input.trim().eq_ignore_ascii_case("yes") {
        eprintln!("[tokf] Skipped. You can register later with `tokf remote setup`.");
        return false;
    }

    match crate::remote_cmd::register_machine(auth) {
        Ok(crate::remote_cmd::RegisterResult::NewlyRegistered {
            machine_id,
            hostname,
        }) => {
            eprintln!("[tokf] Machine registered: {machine_id} ({hostname})");
            true
        }
        Ok(crate::remote_cmd::RegisterResult::AlreadyRegistered {
            machine_id,
            hostname,
        }) => {
            eprintln!("[tokf] Already registered: {machine_id} ({hostname})");
            true
        }
        Err(e) => {
            eprintln!("[tokf] Machine registration failed: {e:#}");
            eprintln!("[tokf] You can try again later with `tokf remote setup`.");
            false
        }
    }
}

/// Ask the user whether to enable automatic usage statistics upload.
fn prompt_usage_stats() {
    eprintln!();
    eprintln!("[tokf] Would you like to automatically upload anonymous usage statistics?");
    eprintln!("[tokf] tokf periodically syncs aggregate token counts (filter name,");
    eprintln!("[tokf] input/output token estimates) in the background. No command content");
    eprintln!("[tokf] or output is ever sent.");
    eprintln!("[tokf] You can change this anytime: `tokf config set sync.upload_stats true|false`");
    eprint!("[tokf] Upload usage statistics? [y/N]: ");
    let _ = std::io::stderr().flush();

    let mut input = String::new();
    if std::io::stdin().lock().read_line(&mut input).is_err() {
        return;
    }
    let enabled =
        input.trim().eq_ignore_ascii_case("y") || input.trim().eq_ignore_ascii_case("yes");

    if let Err(e) = credentials::save_upload_stats_preference(enabled) {
        eprintln!("[tokf] Failed to save preference: {e:#}");
        return;
    }

    if enabled {
        eprintln!("[tokf] Usage statistics upload enabled.");
    } else {
        eprintln!("[tokf] Usage statistics upload disabled.");
    }
}

#[allow(clippy::unnecessary_wraps)] // Returns Result for or_exit() consistency
pub fn cmd_auth_logout() -> anyhow::Result<i32> {
    if credentials::remove() {
        eprintln!("[tokf] Logged out");
    } else {
        eprintln!("[tokf] Not logged in, nothing to do.");
    }
    Ok(0)
}

#[allow(clippy::unnecessary_wraps)] // Returns Result for or_exit() consistency
pub fn cmd_auth_status() -> anyhow::Result<i32> {
    match credentials::load() {
        Some(auth) => {
            println!("Logged in as {}", auth.username);
            println!("Server: {}", auth.server_url);
            if auth.is_expired() {
                println!("Token: expired — run `tokf auth login` to re-authenticate");
            }
        }
        None => {
            println!("Not logged in. Run `tokf auth login` to authenticate.");
        }
    }
    Ok(0)
}

pub fn cmd_auth_delete_account() -> anyhow::Result<i32> {
    let auth = credentials::load()
        .ok_or_else(|| anyhow::anyhow!("not logged in — run `tokf auth login` first"))?;

    eprintln!("[tokf] WARNING: This will permanently delete your account.");
    eprintln!("[tokf] The following data will be removed:");
    eprintln!("[tokf]   - Auth tokens and sessions");
    eprintln!("[tokf]   - Machine registrations and sync state");
    eprintln!("[tokf]   - Usage statistics and event history");
    eprintln!("[tokf]   - Terms of Service acceptance records");
    eprintln!("[tokf] Your published filters will remain available to the community");
    eprintln!("[tokf] with your account converted to unclaimed status.");
    eprintln!();
    eprint!(
        "[tokf] Type your username ({}) to confirm deletion: ",
        auth.username
    );
    std::io::stderr().flush()?;

    let mut input = String::new();
    std::io::stdin().lock().read_line(&mut input)?;
    let input = input.trim();

    if input != auth.username {
        eprintln!("[tokf] Username does not match. Account deletion cancelled.");
        return Ok(1);
    }

    let client = Client::authed()?;
    account_client::delete_account(&client)?;

    // Remove local credentials
    credentials::remove();

    eprintln!("[tokf] Account deleted. Local credentials removed.");
    Ok(0)
}

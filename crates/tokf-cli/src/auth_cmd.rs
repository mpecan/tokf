use std::thread;
use std::time::{Duration, Instant};

use tokf::auth::{client, credentials};

const MAX_NETWORK_RETRIES: u32 = 3;

pub fn cmd_auth_login() -> anyhow::Result<i32> {
    // Check if already logged in
    if let Some(auth) = credentials::load() {
        eprintln!(
            "[tokf] already logged in as {}. Run `tokf auth logout` first.",
            auth.username
        );
        return Ok(0);
    }

    let base_url = client::server_url();

    if !client::is_secure_url(&base_url) {
        eprintln!(
            "[tokf] WARNING: server URL uses insecure HTTP — credentials will be sent unencrypted"
        );
    }

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
    poll_for_token(&http_client, &base_url, &device_resp)
}

fn poll_for_token(
    http_client: &reqwest::blocking::Client,
    base_url: &str,
    device_resp: &client::DeviceFlowResponse,
) -> anyhow::Result<i32> {
    let mut interval = device_resp.interval.clamp(1, 60);
    let expires_in = device_resp.expires_in.clamp(0, 1800);
    let max_attempts = expires_in / interval;
    let mut consecutive_errors: u32 = 0;
    let start = Instant::now();
    let mut last_progress = 0u64;

    for _ in 0..max_attempts {
        thread::sleep(Duration::from_secs(interval.unsigned_abs()));

        match client::poll_token(http_client, base_url, &device_resp.device_code) {
            Ok(client::PollResult::Success(token_resp)) => {
                credentials::save(
                    &token_resp.access_token,
                    &token_resp.user.username,
                    base_url,
                    token_resp.expires_in,
                )?;
                eprintln!();
                eprintln!("[tokf] Logged in as {}", token_resp.user.username);
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

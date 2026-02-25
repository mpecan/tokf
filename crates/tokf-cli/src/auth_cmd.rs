use std::thread;
use std::time::Duration;

use tokf::auth::{client, credentials};

pub fn cmd_auth_login() -> i32 {
    // Check if already logged in
    if let Some((_token, username, _url)) = credentials::load() {
        eprintln!("[tokf] already logged in as {username}. Run `tokf auth logout` first.");
        return 0;
    }

    let base_url = client::server_url();
    let http_client = match reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent(format!("tokf-cli/{}", env!("CARGO_PKG_VERSION")))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[tokf] error: could not create HTTP client: {e}");
            return 1;
        }
    };

    let device_resp = match client::initiate_device_flow(&http_client, &base_url) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[tokf] error: {e}");
            return 1;
        }
    };

    eprintln!("[tokf] Your one-time code: {}", device_resp.user_code);

    // Try to open the verification URI in a browser
    if open::that(&device_resp.verification_uri).is_err() {
        eprintln!(
            "[tokf] Open this URL in your browser: {}",
            device_resp.verification_uri
        );
    }

    eprintln!("[tokf] Waiting for authorization...");
    poll_for_token(&http_client, &base_url, &device_resp)
}

fn poll_for_token(
    http_client: &reqwest::blocking::Client,
    base_url: &str,
    device_resp: &client::DeviceFlowResponse,
) -> i32 {
    let mut interval = device_resp.interval.max(1);
    let max_attempts = device_resp.expires_in / interval;

    for _ in 0..max_attempts {
        thread::sleep(Duration::from_secs(interval.unsigned_abs()));

        match client::poll_token(http_client, base_url, &device_resp.device_code) {
            Ok(client::PollResult::Success(token_resp)) => {
                if let Err(e) = credentials::save(
                    &token_resp.access_token,
                    &token_resp.user.username,
                    base_url,
                ) {
                    eprintln!("[tokf] error: could not save credentials: {e}");
                    return 1;
                }
                eprintln!("[tokf] Logged in as {}", token_resp.user.username);
                return 0;
            }
            Ok(client::PollResult::Pending { .. }) => {
                eprint!(".");
            }
            Ok(client::PollResult::SlowDown {
                interval: new_interval,
            }) => {
                interval = new_interval;
                eprint!(".");
            }
            Ok(client::PollResult::Failed(msg)) => {
                eprintln!();
                if msg.contains("denied") {
                    eprintln!("[tokf] error: authorization was denied");
                } else {
                    eprintln!("[tokf] error: {msg}");
                }
                return 1;
            }
            Err(e) => {
                eprintln!();
                eprintln!("[tokf] error: {e}");
                return 1;
            }
        }
    }

    eprintln!();
    eprintln!("[tokf] error: authorization timed out — run `tokf auth login` again");
    1
}

pub fn cmd_auth_logout() -> i32 {
    credentials::remove();
    eprintln!("[tokf] Logged out");
    0
}

pub fn cmd_auth_status() -> i32 {
    match credentials::load() {
        Some((_token, username, server_url)) => {
            println!("Logged in as {username}");
            println!("Server: {server_url}");
        }
        None => {
            println!("Not logged in");
        }
    }
    0
}

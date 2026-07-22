use reqwest::blocking::multipart::Form;

use crate::auth::credentials;
use crate::runtime::Runtime;

use super::{RemoteError, classify_reqwest_error, require_success};

/// Load stored auth credentials and validate they are not expired.
///
/// # Errors
///
/// Returns an error if credentials are absent or the token has expired.
pub fn load_auth(rt: &Runtime) -> anyhow::Result<credentials::LoadedAuth> {
    let auth = credentials::load(rt)
        .ok_or_else(|| anyhow::anyhow!("not logged in — run `tokf auth login` first"))?;
    if auth.is_expired() {
        anyhow::bail!("token has expired — run `tokf auth login` to re-authenticate");
    }
    Ok(auth)
}

/// Centralized HTTP client for all tokf remote operations.
///
/// Handles timeouts (`TOKF_HTTP_TIMEOUT`, default 5s), auth header injection,
/// error classification into [`RemoteError`], and one automatic retry on
/// transient failures for idempotent methods (GET/PUT).
pub struct Client {
    inner: reqwest::blocking::Client,
    base_url: String,
    token: Option<String>,
    /// `TOKF_DEBUG`, captured so errors built here can render their verbose
    /// form without reaching back into the environment.
    debug: bool,
}

impl Client {
    /// Build a client with the given base URL and optional auth token.
    ///
    /// Reads `TOKF_HTTP_TIMEOUT` for the request timeout (default 5s).
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying HTTP client cannot be constructed.
    pub fn new(rt: &Runtime, base_url: &str, token: Option<&str>) -> anyhow::Result<Self> {
        let timeout = rt.http_timeout();
        let inner = reqwest::blocking::Client::builder()
            .timeout(timeout)
            .connect_timeout(timeout)
            .redirect(reqwest::redirect::Policy::limited(10))
            .build()
            .map_err(|e| anyhow::anyhow!("could not build HTTP client: {e}"))?;
        Ok(Self {
            inner,
            base_url: base_url.trim_end_matches('/').to_string(),
            token: token.map(String::from),
            debug: rt.debug(),
        })
    }

    /// Build an authenticated client from stored credentials.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing/expired or the client
    /// cannot be constructed.
    pub fn authed(rt: &Runtime) -> anyhow::Result<Self> {
        let auth = load_auth(rt)?;
        Self::new(rt, &auth.server_url, Some(&auth.token))
    }

    /// Build an unauthenticated client for the given base URL.
    ///
    /// Used for public endpoints (global gain, auth flow).
    ///
    /// # Errors
    ///
    /// Returns an error if the client cannot be constructed.
    pub fn unauthenticated(rt: &Runtime, base_url: &str) -> anyhow::Result<Self> {
        Self::new(rt, base_url, None)
    }

    /// The base URL this client is configured for.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Build the full URL for a request path.
    ///
    /// Panics in debug builds if `path` doesn't start with `/`.
    fn url(&self, path: &str) -> String {
        debug_assert!(
            path.starts_with('/'),
            "Client path must start with '/': {path}"
        );
        format!("{}{path}", self.base_url)
    }

    /// GET `{base_url}{path}` and deserialize JSON.
    ///
    /// Retries once on transient errors (connection/timeout/5xx).
    ///
    /// # Errors
    ///
    /// Returns an error on network failure, non-2xx status, or JSON parse error.
    pub fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> anyhow::Result<T> {
        let url = self.url(path);
        let resp = self.execute_idempotent(|c| c.inner.get(&url), &url)?;
        resp.json::<T>()
            .map_err(|e| anyhow::anyhow!("invalid response from server: {e}"))
    }

    /// GET `{base_url}{path}` with query parameters and deserialize JSON.
    ///
    /// Retries once on transient errors.
    ///
    /// # Errors
    ///
    /// Returns an error on network failure, non-2xx status, or JSON parse error.
    pub fn get_with_query<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        params: &[(&str, &str)],
    ) -> anyhow::Result<T> {
        let url = self.url(path);
        let resp = self.execute_idempotent(|c| c.inner.get(&url).query(params), &url)?;
        resp.json::<T>()
            .map_err(|e| anyhow::anyhow!("invalid response from server: {e}"))
    }

    /// GET `{base_url}{path}` and return the raw response.
    ///
    /// Retries once on transient errors. Useful for non-JSON endpoints.
    ///
    /// # Errors
    ///
    /// Returns an error on network failure or non-2xx status.
    pub fn get_raw(&self, path: &str) -> anyhow::Result<reqwest::blocking::Response> {
        let url = self.url(path);
        self.execute_idempotent(|c| c.inner.get(&url), &url)
    }

    /// POST `{base_url}{path}` with a JSON body and deserialize the response.
    ///
    /// Does **not** retry — POST is non-idempotent.
    ///
    /// # Errors
    ///
    /// Returns an error on network failure, non-2xx status, or JSON parse error.
    pub fn post<B: serde::Serialize, T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> anyhow::Result<T> {
        let url = self.url(path);
        let resp = Self::send_once(
            self.build_request(self.inner.post(&url)).json(body),
            &url,
            self.debug,
        )?;
        resp.json::<T>()
            .map_err(|e| anyhow::anyhow!("invalid response from server: {e}"))
    }

    /// DELETE `{base_url}{path}`, returning the raw response.
    ///
    /// Does **not** retry — DELETE is non-idempotent in our usage
    /// (account deletion is a one-shot operation).
    ///
    /// # Errors
    ///
    /// Returns an error on network failure or non-2xx status.
    pub fn delete(&self, path: &str) -> anyhow::Result<reqwest::blocking::Response> {
        let url = self.url(path);
        Self::send_once(
            self.build_request(self.inner.delete(&url)),
            &url,
            self.debug,
        )
    }

    /// POST `{base_url}{path}` with a multipart form.
    ///
    /// Returns the raw response — callers handle per-status-code logic (e.g.
    /// 400 test failures in publish). Does not retry (POST is non-idempotent).
    ///
    /// # Errors
    ///
    /// Returns an error on network failure (does not check status).
    pub fn post_multipart<F>(
        &self,
        path: &str,
        form_builder: F,
    ) -> anyhow::Result<reqwest::blocking::Response>
    where
        F: FnOnce() -> Form,
    {
        let url = self.url(path);
        let form = form_builder();
        self.build_request(self.inner.post(&url))
            .multipart(form)
            .send()
            .map_err(|e| anyhow::Error::from(classify_reqwest_error(&url, e, self.debug)))
    }

    /// PUT `{base_url}{path}` with a multipart form.
    ///
    /// Returns the raw response — callers handle per-status-code logic.
    /// Retries once on transport-level errors (PUT is idempotent).
    /// `form_builder` must be callable multiple times (forms are consumed on send).
    ///
    /// # Errors
    ///
    /// Returns an error on network failure (does not check status).
    pub fn put_multipart<F>(
        &self,
        path: &str,
        form_builder: F,
    ) -> anyhow::Result<reqwest::blocking::Response>
    where
        F: Fn() -> Form,
    {
        let url = self.url(path);
        let result = self
            .build_request(self.inner.put(&url))
            .multipart(form_builder())
            .send();

        match result {
            Ok(resp) => Ok(resp),
            Err(first_err) => {
                let classified = classify_reqwest_error(&url, first_err, self.debug);
                if classified.is_transient() {
                    // Retry once on transport errors.
                    self.build_request(self.inner.put(&url))
                        .multipart(form_builder())
                        .send()
                        .map_err(|e| {
                            anyhow::Error::from(classify_reqwest_error(&url, e, self.debug))
                        })
                } else {
                    Err(classified.into())
                }
            }
        }
    }

    // ── Internal helpers ────────────────────────────────────────────────

    /// Add auth header if a token is present.
    fn build_request(
        &self,
        builder: reqwest::blocking::RequestBuilder,
    ) -> reqwest::blocking::RequestBuilder {
        match &self.token {
            Some(token) => builder.header("Authorization", format!("Bearer {token}")),
            None => builder,
        }
    }

    /// Execute an idempotent request (GET/PUT), retrying once on transient errors.
    fn execute_idempotent<F>(
        &self,
        make_request: F,
        url: &str,
    ) -> anyhow::Result<reqwest::blocking::Response>
    where
        F: Fn(&Self) -> reqwest::blocking::RequestBuilder,
    {
        let req = self.build_request(make_request(self));
        match send_and_classify(req, url, self.debug) {
            Ok(resp) => Ok(resp),
            Err(err) if err.is_transient() => {
                let req = self.build_request(make_request(self));
                Self::send_once(req, url, self.debug)
            }
            Err(err) => Err(err.into()),
        }
    }

    /// Send a request once (no retry). Converts to `anyhow::Error`.
    fn send_once(
        request: reqwest::blocking::RequestBuilder,
        url: &str,
        debug: bool,
    ) -> anyhow::Result<reqwest::blocking::Response> {
        send_and_classify(request, url, debug).map_err(anyhow::Error::from)
    }
}

/// Send a request, classify errors into [`RemoteError`], and check the response status.
fn send_and_classify(
    request: reqwest::blocking::RequestBuilder,
    url: &str,
    debug: bool,
) -> Result<reqwest::blocking::Response, RemoteError> {
    let resp = request
        .send()
        .map_err(|e| classify_reqwest_error(url, e, debug))?;
    require_success(resp, url, debug)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn timeout_comes_from_the_runtime() {
        // The isolated runtime carries the documented default...
        let rt = Runtime::isolated();
        assert_eq!(
            rt.http_timeout(),
            std::time::Duration::from_secs(crate::runtime::DEFAULT_TIMEOUT_SECS)
        );

        // ...and an override is honoured without touching the environment.
        let rt = Runtime::builder()
            .http_timeout(std::time::Duration::from_secs(42))
            .build();
        assert_eq!(rt.http_timeout(), std::time::Duration::from_secs(42));
    }

    #[test]
    fn client_base_url_trims_trailing_slash() {
        let rt = Runtime::isolated();
        let c = Client::new(&rt, "https://api.tokf.net/", None).unwrap();
        assert_eq!(c.base_url(), "https://api.tokf.net");
    }

    #[test]
    fn client_base_url_no_trailing_slash() {
        let rt = Runtime::isolated();
        let c = Client::new(&rt, "https://api.tokf.net", None).unwrap();
        assert_eq!(c.base_url(), "https://api.tokf.net");
    }

    #[test]
    fn client_unauthenticated_has_no_token() {
        let rt = Runtime::isolated();
        let c = Client::unauthenticated(&rt, "https://api.tokf.net").unwrap();
        assert!(c.token.is_none());
    }

    #[test]
    fn client_new_with_token() {
        let rt = Runtime::isolated();
        let c = Client::new(&rt, "https://api.tokf.net", Some("tok123")).unwrap();
        assert_eq!(c.token.as_deref(), Some("tok123"));
    }

    #[test]
    fn url_helper_formats_correctly() {
        let rt = Runtime::isolated();
        let c = Client::new(&rt, "https://api.tokf.net/", None).unwrap();
        assert_eq!(c.url("/api/test"), "https://api.tokf.net/api/test");
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "must start with '/'")]
    fn url_helper_panics_on_missing_slash_in_debug() {
        let rt = Runtime::isolated();
        let c = Client::new(&rt, "https://api.tokf.net", None).unwrap();
        let _ = c.url("api/test");
    }
}

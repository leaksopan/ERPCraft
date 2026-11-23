//! The HTTP side of the connector: OAuth2 client credentials and JSON calls to the BC API.
//!
//! The client secret comes from the environment and the bearer token lives only in this struct;
//! neither is stored, logged or put into an error message (every message is scrubbed of both).

use serde_json::Value as Json;

use super::Settings;
use crate::error::{EngineError, ErrorKind, Result};

/// Environment variable holding the Microsoft Entra app's client secret.
pub const SECRET_ENV: &str = "ERPCRAFT_BC_CLIENT_SECRET";

static SECRET_OVERRIDE: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Sets an in-process client secret override (used by tests to avoid unsafe `std::env::set_var`).
#[doc(hidden)]
pub fn set_secret_override(secret: &str) {
    let _ = SECRET_OVERRIDE.set(secret.to_string());
}

pub(crate) fn client_secret() -> Option<String> {
    SECRET_OVERRIDE
        .get()
        .cloned()
        .or_else(|| std::env::var(SECRET_ENV).ok())
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

#[cfg(feature = "bc")]
pub use http::Client;

#[cfg(not(feature = "bc"))]
pub use stub::Client;

/// Whether a failed call means the next call will fail the same way (bad credentials, network
/// down), so a batch should stop instead of trying every record.
pub fn is_fatal(e: &EngineError) -> bool {
    matches!(e.kind, ErrorKind::Permission | ErrorKind::Io)
}

#[cfg(feature = "bc")]
mod http {
    use std::time::Duration;

    use serde_json::{Map, Value as Json};
    use ureq::Agent;

    use super::{SECRET_ENV, Settings};
    use crate::bc::url_segment;
    use crate::error::{EngineError, Result};

    const SCOPE: &str = "https://api.businesscentral.dynamics.com/.default";
    /// Largest response body read (one page of an entity set is far smaller).
    const MAX_BODY: u64 = 64 * 1024 * 1024;
    const TIMEOUT: Duration = Duration::from_secs(120);
    /// Longest error text taken from a response.
    const MAX_MESSAGE: usize = 400;

    pub struct Client {
        agent: Agent,
        token: String,
        secret: String,
        /// `{API Base URL}/{tenant}/{environment}/api/v2.0`
        root: String,
    }

    type Reply = std::result::Result<ureq::http::Response<ureq::Body>, ureq::Error>;

    impl Client {
        /// Gets a token with the client-credentials grant.
        pub fn connect(s: &Settings) -> Result<Client> {
            let secret = super::client_secret().ok_or_else(|| {
                EngineError::invalid(format!(
                    "The client secret is not set. Set the environment variable {SECRET_ENV} to the secret of the Microsoft Entra app (it is never stored in the company)."
                ))
            })?;
            check_url("Token URL", &s.token_url)?;
            check_url("API Base URL", &s.api_base)?;
            let agent: Agent = Agent::config_builder()
                .http_status_as_error(false)
                .max_redirects(0)
                .timeout_global(Some(TIMEOUT))
                .build()
                .new_agent();
            let root = format!("{}/{}/{}/api/v2.0", s.api_base, url_segment(&s.tenant), url_segment(&s.environment));
            let mut c = Client { agent, token: String::new(), secret, root };
            let reply = c.agent.post(&s.token_url).header("Accept", "application/json").send_form([
                ("grant_type", "client_credentials"),
                ("client_id", s.client_id.as_str()),
                ("client_secret", c.secret.as_str()),
                ("scope", SCOPE),
            ]);
            let body = c.finish(reply, "token request")?;
            let token = body.get("access_token").and_then(Json::as_str).map(str::trim).unwrap_or("");
            if token.is_empty() || token.len() > 64 * 1024 || token.chars().any(|ch| ch.is_control() || ch == ' ') {
                return Err(EngineError::invalid("The token endpoint did not return a usable access token."));
            }
            c.token = token.to_string();
            Ok(c)
        }

        pub fn companies_url(&self) -> String {
            format!("{}/companies", self.root)
        }

        /// `…/companies({company})/{entity}`; the company must be a GUID.
        pub fn entity_url(&self, company: &str, entity: &str) -> Result<String> {
            if !is_guid(company) {
                return Err(EngineError::invalid(format!(
                    "Company ID '{}' is not a GUID. Run bc.test_connection to list the companies and copy the id.",
                    super::super::map::truncate(company, 60)
                )));
            }
            Ok(format!("{}/companies({company})/{entity}", self.root))
        }

        /// `…/{entity}({id})` for a record id returned by the API.
        pub fn record_url(&self, entity_url: &str, id: &str) -> Result<String> {
            if !is_guid(id) {
                return Err(EngineError::invalid("Business Central returned a record id that is not a GUID."));
            }
            Ok(format!("{entity_url}({id})"))
        }

        /// GET a URL under the API root (also used for `@odata.nextLink`, which must not lead
        /// anywhere else: the bearer token goes with it).
        pub fn get(&self, url: &str) -> Result<Json> {
            self.check_under_root(url)?;
            let reply = self.agent.get(url).header("Authorization", &self.bearer()).header("Accept", "application/json").call();
            self.finish(reply, "request")
        }

        pub fn post(&self, url: &str, body: &Map<String, Json>) -> Result<Json> {
            self.check_under_root(url)?;
            let reply = self.agent.post(url).header("Authorization", &self.bearer()).header("Accept", "application/json").send_json(body);
            self.finish(reply, "create")
        }

        pub fn patch(&self, url: &str, etag: &str, body: &Map<String, Json>) -> Result<Json> {
            self.check_under_root(url)?;
            let etag = if etag.is_empty() || etag.chars().any(char::is_control) { "*" } else { etag };
            let reply = self
                .agent
                .patch(url)
                .header("Authorization", &self.bearer())
                .header("Accept", "application/json")
                .header("If-Match", etag)
                .send_json(body);
            self.finish(reply, "update")
        }

        fn bearer(&self) -> String {
            format!("Bearer {}", self.token)
        }

        fn check_under_root(&self, url: &str) -> Result<()> {
            if url.starts_with(&format!("{}/", self.root)) && !url.chars().any(|c| c.is_control() || c == ' ') {
                Ok(())
            } else {
                Err(EngineError::refused("Business Central returned a link outside the configured API; it was not followed."))
            }
        }

        /// The JSON body of a 2xx reply, or an error that names the status and BC's message.
        fn finish(&self, reply: Reply, what: &str) -> Result<Json> {
            let mut resp = match reply {
                Ok(r) => r,
                Err(e) => return Err(EngineError::io(self.scrub(&format!("Could not reach Business Central ({what}): {e}")))),
            };
            let status = resp.status().as_u16();
            let text = resp.body_mut().with_config().limit(MAX_BODY).read_to_string();
            let text = match text {
                Ok(t) => t,
                Err(e) => return Err(EngineError::io(self.scrub(&format!("Could not read the Business Central reply ({what}): {e}")))),
            };
            if (200..300).contains(&status) {
                if text.trim().is_empty() {
                    return Ok(Json::Null);
                }
                return serde_json::from_str(&text)
                    .map_err(|_| EngineError::invalid(format!("Business Central sent a reply that is not JSON ({what}).")));
            }
            let detail = error_detail(&text);
            let mut msg = format!("Business Central {what} failed: HTTP {status}");
            if !detail.is_empty() {
                msg.push_str(": ");
                msg.push_str(&super::super::map::truncate(&detail, MAX_MESSAGE));
            }
            let msg = self.scrub(&msg);
            Err(match status {
                401 | 403 => EngineError::permission(format!(
                    "{msg}. Check Tenant ID, Client ID and {SECRET_ENV}, and that the app is registered in Business Central's Microsoft Entra Applications page with permission sets."
                )),
                400 => EngineError::invalid(msg),
                404 => EngineError::not_found(msg),
                409 | 412 => EngineError::conflict(msg),
                _ if what == "token request" => EngineError::permission(msg),
                _ if status >= 500 || status == 429 => EngineError::io(msg),
                _ => EngineError::refused(msg),
            })
        }

        /// `msg` without the secret or the token.
        fn scrub(&self, msg: &str) -> String {
            let mut out = msg.to_string();
            for s in [&self.secret, &self.token] {
                if s.len() >= 4 {
                    out = out.replace(s.as_str(), "***");
                }
            }
            out
        }
    }

    /// OData `{"error": {"code", "message"}}` or OAuth `{"error", "error_description"}`.
    fn error_detail(text: &str) -> String {
        let Ok(j) = serde_json::from_str::<Json>(text) else {
            return text.chars().filter(|c| !c.is_control()).take(MAX_MESSAGE).collect();
        };
        let e = j.get("error");
        if let Some(obj) = e.filter(|e| e.is_object()) {
            let code = obj.get("code").and_then(Json::as_str).unwrap_or("");
            let message = obj.get("message").and_then(Json::as_str).unwrap_or("");
            return [code, message].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(" ");
        }
        let code = e.and_then(Json::as_str).unwrap_or("");
        let desc = j.get("error_description").and_then(Json::as_str).unwrap_or("");
        let desc = desc.lines().next().unwrap_or("");
        [code, desc].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(" ")
    }

    /// Only https, except for a server on this machine (tests, proxies).
    fn check_url(name: &str, url: &str) -> Result<()> {
        let local = ["http://127.0.0.1", "http://localhost", "http://[::1]"]
            .iter()
            .any(|p| url.strip_prefix(p).is_some_and(|rest| rest.is_empty() || rest.starts_with([':', '/'])));
        if (url.starts_with("https://") || local) && !url.chars().any(|c| c.is_control() || c == ' ') {
            Ok(())
        } else {
            Err(EngineError::invalid(format!("{name} must be an https:// URL.")))
        }
    }

    fn is_guid(s: &str) -> bool {
        let parts: Vec<&str> = s.split('-').collect();
        parts.iter().map(|p| p.len()).eq([8, 4, 4, 4, 12]) && parts.iter().all(|p| p.chars().all(|c| c.is_ascii_hexdigit()))
    }
}

#[cfg(not(feature = "bc"))]
mod stub {
    use serde_json::{Map, Value as Json};

    use super::Settings;
    use crate::error::{EngineError, Result};

    /// This build has no HTTP client: every call says so.
    pub struct Client;

    fn none() -> EngineError {
        EngineError::invalid("This build of ERPCraft has no Business Central connector (built without the 'bc' feature).")
    }

    impl Client {
        pub fn connect(_: &Settings) -> Result<Client> {
            Err(none())
        }
        pub fn companies_url(&self) -> String {
            String::new()
        }
        pub fn entity_url(&self, _: &str, _: &str) -> Result<String> {
            Err(none())
        }
        pub fn record_url(&self, _: &str, _: &str) -> Result<String> {
            Err(none())
        }
        pub fn get(&self, _: &str) -> Result<Json> {
            Err(none())
        }
        pub fn post(&self, _: &str, _: &Map<String, Json>) -> Result<Json> {
            Err(none())
        }
        pub fn patch(&self, _: &str, _: &str, _: &Map<String, Json>) -> Result<Json> {
            Err(none())
        }
    }
}

/// Every row of an entity set, following `@odata.nextLink`, up to `cap` rows. Returns the rows and
/// whether the cap (or the page limit) cut the list short.
pub fn fetch_all(c: &Client, url: &str, cap: usize) -> Result<(Vec<Json>, bool)> {
    const MAX_PAGES: usize = 2_000;
    let mut out = Vec::new();
    let mut next = Some(url.to_string());
    let mut pages = 0usize;
    while let Some(u) = next.take() {
        pages += 1;
        if pages > MAX_PAGES {
            return Ok((out, true));
        }
        let page = c.get(&u)?;
        let Some(rows) = page.get("value").and_then(Json::as_array) else {
            return Err(EngineError::invalid("Business Central returned a reply without a 'value' list."));
        };
        for r in rows {
            if out.len() >= cap {
                return Ok((out, true));
            }
            if r.is_object() {
                out.push(r.clone());
            }
        }
        next = page.get("@odata.nextLink").and_then(Json::as_str).map(str::to_string);
    }
    Ok((out, false))
}

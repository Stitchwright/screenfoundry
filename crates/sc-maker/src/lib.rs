//! ScreenFoundry's side of [MakerFoundry](https://makerfoundry.app) (SC-11): the requests the app
//! sends, how it reads the answers, and the checks on every address that came from outside. There
//! is no network code here: the host (the desktop app's HTTP client, the browser's `fetch`) sends
//! a [`Request`] and hands back a [`Response`], so the same code runs on both and is tested without
//! a network.
//!
//! The contract is MakerFoundry's `docs/api.md` (how publishing from an app works, Open in ...)
//! and the Foundry Account service's `/v1/signup`, `/v1/signin`, `/v1/account` and `/v1/signout`.
//! The design notes and what was checked are in `docs/makerfoundry.md`.
//!
//! - [`publish`]: send a design as a private draft and read the answer;
//! - [`account`]: Foundry Account sign-in, sign-up, sign-out and the account's credits;
//! - [`link`]: the `?open=` address of a design to open, strictly checked, and the name of the file.
#![forbid(unsafe_code)]

pub mod account;
pub mod link;
pub mod publish;
mod text;

/// An HTTP request for the host to send: no cookies, no redirects (the host follows neither).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub method: &'static str,
    pub url: String,
    /// Header name and value pairs.
    pub headers: Vec<(&'static str, String)>,
    pub body: String,
}

impl Request {
    /// Whether this is one of the few requests the app ever makes: a `POST` of JSON to MakerFoundry's
    /// design endpoint or one of the Foundry Account service's four, with exactly the headers each
    /// takes. A host sends only requests that pass, so a mistake elsewhere in the app can't make it
    /// send anything else, or a token anywhere else.
    pub fn expected(&self) -> Result<(), String> {
        let signed = if self.url == format!("{}/api/v1/designs", Site::MAKERFOUNDRY) {
            true
        } else {
            let path = self.url.strip_prefix(Site::ACCOUNTS).unwrap_or_default();
            match path {
                "/v1/signin" | "/v1/signup" => false,
                "/v1/signout" | "/v1/account" => true,
                _ => return Err("this app sends nothing to that address".into()),
            }
        };
        if self.method != "POST" || self.body.len() > MAX_BODY {
            return Err("this app sends nothing like that".into());
        }
        let mut auth = false;
        let mut json = false;
        for (name, value) in &self.headers {
            match *name {
                "Authorization" if signed && !auth => {
                    auth = value.strip_prefix("Bearer ").is_some_and(|t| publish::session_token(t).is_ok());
                    if !auth {
                        return Err("only a Foundry Account sign-in is ever sent".into());
                    }
                }
                "Content-Type" if !json && value == "application/json" => json = true,
                _ => return Err("this app sends no other headers".into()),
            }
        }
        if auth != signed || !json {
            return Err("the request is missing what that address needs".into());
        }
        Ok(())
    }
}

/// The most a request body may be: an 8 MB design as base64, with its wrapping.
pub const MAX_BODY: usize = 11 * 1024 * 1024;

/// What the host got back: the status and the body as text (empty when there was none).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub body: String,
}

/// A service the app talks to: an `https` origin (no path), or `http` on this machine for
/// development.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Site {
    origin: String,
}

impl Site {
    /// MakerFoundry.
    pub const MAKERFOUNDRY: &'static str = "https://makerfoundry.app";
    /// The Foundry Account service (Foundry sign-in is shared by the whole family).
    pub const ACCOUNTS: &'static str = "https://cloud.stitchfoundry.app";

    /// MakerFoundry's site.
    pub fn makerfoundry() -> Self {
        Self { origin: Self::MAKERFOUNDRY.to_string() }
    }

    /// The Foundry Account service's site.
    pub fn accounts() -> Self {
        Self { origin: Self::ACCOUNTS.to_string() }
    }

    /// The site at `origin`: `https://host`, or `http://127.0.0.1:port` / `http://localhost:port`
    /// to try a service on this machine. No user name, path, query or fragment.
    pub fn parse(origin: &str) -> Result<Self, String> {
        let origin = origin.trim().trim_end_matches('/');
        let (scheme, rest) = origin.split_once("://").ok_or("a site's address starts with https://")?;
        let bad =
            rest.is_empty() || rest.contains(['/', '?', '#', '@', '\\']) || !rest.is_ascii() || rest.chars().any(|c| c.is_control() || c == ' ');
        if bad {
            return Err("a site's address is just its host, like https://makerfoundry.app".into());
        }
        let host = rest.split(':').next().unwrap_or_default();
        let local = host == "127.0.0.1" || host == "localhost";
        match scheme.to_ascii_lowercase().as_str() {
            "https" => {}
            "http" if local => {}
            _ => return Err("a site's address starts with https://".into()),
        }
        Ok(Self { origin: format!("{}://{}", scheme.to_ascii_lowercase(), rest.to_ascii_lowercase()) })
    }

    /// The address without a closing slash.
    pub fn origin(&self) -> &str {
        &self.origin
    }

    /// The address of `path` (which starts with `/`) on this site.
    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.origin)
    }

    /// Whether `address` is a page on this very site: the same scheme and host, no user name, and
    /// a path (or nothing) after it.
    pub fn owns(&self, address: &str) -> bool {
        let Some(rest) = address.strip_prefix(self.origin.as_str()) else { return false };
        !address.chars().any(|c| c.is_control() || c == ' ' || c == '\\') && (rest.is_empty() || rest.starts_with(['/', '?']))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_site_is_a_bare_origin() {
        assert_eq!(Site::parse("https://makerfoundry.app/").unwrap(), Site::makerfoundry());
        assert_eq!(Site::parse("HTTPS://MakerFoundry.app").unwrap(), Site::makerfoundry());
        assert!(Site::parse("http://127.0.0.1:8080").is_ok() && Site::parse("http://localhost:3000").is_ok());
        for bad in [
            "",
            "makerfoundry.app",
            "http://makerfoundry.app",
            "ftp://makerfoundry.app",
            "https://",
            "https://makerfoundry.app/api",
            "https://user@makerfoundry.app",
            "https://makerfoundry.app?x=1",
            "https://makerfoundry.app\\evil.com",
            "https://makerfoundry.app evil.com",
            "https://märkerfoundry.app",
            "http://localhost.evil.com",
        ] {
            assert!(Site::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_host_sends_only_the_requests_the_app_makes() {
        let publish = publish::Publish::new(Site::makerfoundry(), "a.svg", "PHN2Zy8+".into(), "x").unwrap();
        let accounts = account::Accounts::new(Site::accounts());
        let good = [
            publish.request("sfs_t").unwrap(),
            accounts.sign_in("me@example.com", "pw").unwrap(),
            accounts.sign_up("me@example.com", "pw").unwrap(),
            accounts.account("sfs_t").unwrap(),
            accounts.sign_out("sfs_t").unwrap(),
        ];
        for r in &good {
            assert_eq!(r.expected(), Ok(()), "{}", r.url);
        }
        let mut bad = vec![];
        for r in &good {
            let mut x = r.clone();
            x.method = "GET";
            bad.push(x);
            let mut x = r.clone();
            x.headers.push(("Cookie", "a=b".into()));
            bad.push(x);
            let mut x = r.clone();
            x.headers.push(("X-Extra", "1".into()));
            bad.push(x);
            let mut x = r.clone();
            x.headers.retain(|(k, _)| *k != "Content-Type");
            bad.push(x);
            let mut x = r.clone();
            x.body = "x".repeat(MAX_BODY + 1);
            bad.push(x);
        }
        for url in [
            "https://makerfoundry.app/api/v1/designs/1",
            "https://makerfoundry.app/api/v1/account",
            "https://makerfoundry.app.evil.com/api/v1/designs",
            "https://evil.com/v1/signin",
            "https://cloud.stitchfoundry.app/v1/checkout",
            "https://cloud.stitchfoundry.app/v1/signin?x=1",
            "http://makerfoundry.app/api/v1/designs",
            "http://127.0.0.1:1/api/v1/designs",
            "",
        ] {
            let mut x = good[0].clone();
            x.url = url.into();
            bad.push(x);
        }
        // a token sent where none belongs, a password-less request signed, a foreign token
        let mut x = good[1].clone();
        x.headers.push(("Authorization", "Bearer sfs_t".into()));
        bad.push(x);
        let mut x = good[0].clone();
        x.headers.retain(|(k, _)| *k != "Authorization");
        bad.push(x);
        for token in ["Bearer sk-ant-api03-secret", "Bearer ", "sfs_t", "Basic sfs_t", "Bearer sfs_"] {
            let mut x = good[0].clone();
            x.headers.retain(|(k, _)| *k != "Authorization");
            x.headers.push(("Authorization", token.into()));
            bad.push(x);
        }
        let mut x = good[0].clone();
        x.headers.push(("Authorization", "Bearer sfs_t".into()));
        bad.push(x);
        for r in &bad {
            assert!(r.expected().is_err(), "{r:?}");
        }
    }

    #[test]
    fn owns_only_pages_on_the_same_site() {
        let s = Site::makerfoundry();
        assert!(s.owns("https://makerfoundry.app/designs/42") && s.owns("https://makerfoundry.app"));
        for other in [
            "https://makerfoundry.app.evil.com/designs/42",
            "https://makerfoundry.appevil.com/",
            "https://makerfoundry.app@evil.com/designs/42",
            "https://makerfoundry.app:8443/designs/42",
            "http://makerfoundry.app/designs/42",
            "https://www.makerfoundry.app/designs/42",
            "//evil.com/designs/42",
            "/designs/42",
            "javascript:alert(1)",
            "https://makerfoundry.app/designs/4 2",
            "https://makerfoundry.app/designs/42\r\nSet-Cookie: a=b",
            "https://makerfoundry.app\\@evil.com/",
        ] {
            assert!(!s.owns(other), "{other:?}");
        }
    }
}

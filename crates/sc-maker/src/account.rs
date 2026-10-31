//! Foundry Account: the family's shared sign-in (`cloud.stitchfoundry.app`). Signing up sends no
//! email and gives 100 credits. The session token (`sfs_...`) is what MakerFoundry takes in place
//! of a password; the password is sent once, to the account service, and forgotten.

use serde_json::json;

use crate::publish::{refusal, session_token};
use crate::{Request, Response, Site};

/// A signed-in Foundry Account.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    pub token: String,
    pub email: String,
    pub credits: Option<u64>,
}

/// The account service's requests, for `site`.
#[derive(Clone, Debug)]
pub struct Accounts {
    site: Site,
}

impl Accounts {
    pub fn new(site: Site) -> Self {
        Self { site }
    }

    /// Make an account and sign in to it.
    pub fn sign_up(&self, email: &str, password: &str) -> Result<Request, String> {
        self.credentials("/v1/signup", email, password)
    }

    /// Sign in to an account.
    pub fn sign_in(&self, email: &str, password: &str) -> Result<Request, String> {
        self.credentials("/v1/signin", email, password)
    }

    /// End the session on the service.
    pub fn sign_out(&self, token: &str) -> Result<Request, String> {
        self.signed("/v1/signout", token)
    }

    /// Read the account (its email and credits).
    pub fn account(&self, token: &str) -> Result<Request, String> {
        self.signed("/v1/account", token)
    }

    /// The session in a sign-up or sign-in answer.
    pub fn session(&self, response: &Response) -> Result<Session, String> {
        if !(200..300).contains(&response.status) {
            return Err(refusal(response));
        }
        let v: serde_json::Value = serde_json::from_str(&response.body).map_err(|_| "the account service's answer wasn't readable".to_string())?;
        let token = v.get("token").and_then(serde_json::Value::as_str).ok_or("the account service sent no sign-in")?;
        let token = session_token(token).map_err(|_| "the account service sent a sign-in this app can't use".to_string())?;
        Ok(Session { token: token.to_string(), email: email_of(v.get("account")), credits: credits_of(v.get("account")) })
    }

    /// The account in a read-account answer, keeping the `token` it was read with.
    pub fn read(&self, token: &str, response: &Response) -> Result<Session, String> {
        if !(200..300).contains(&response.status) {
            return Err(refusal(response));
        }
        let v: serde_json::Value = serde_json::from_str(&response.body).map_err(|_| "the account service's answer wasn't readable".to_string())?;
        Ok(Session { token: session_token(token)?.to_string(), email: email_of(Some(&v)), credits: credits_of(Some(&v)) })
    }

    fn credentials(&self, path: &str, email: &str, password: &str) -> Result<Request, String> {
        let email = email.trim();
        if email.is_empty() || email.len() > 254 || email.chars().any(|c| c.is_control()) || !email.contains('@') {
            return Err("type the email address of your Foundry Account".into());
        }
        if password.is_empty() || password.len() > 1024 {
            return Err("type your Foundry Account password".into());
        }
        Ok(Request {
            method: "POST",
            url: self.site.url(path),
            headers: vec![("Content-Type", "application/json".into())],
            body: json!({ "email": email, "password": password }).to_string(),
        })
    }

    fn signed(&self, path: &str, token: &str) -> Result<Request, String> {
        let token = session_token(token)?;
        Ok(Request {
            method: "POST",
            url: self.site.url(path),
            headers: vec![("Authorization", format!("Bearer {token}")), ("Content-Type", "application/json".into())],
            body: "{}".into(),
        })
    }
}

fn email_of(account: Option<&serde_json::Value>) -> String {
    account.and_then(|a| a.get("email")).and_then(serde_json::Value::as_str).map(crate::text::sentence).unwrap_or_default()
}

fn credits_of(account: Option<&serde_json::Value>) -> Option<u64> {
    account.and_then(|a| a.get("credits")).and_then(serde_json::Value::as_u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn accounts() -> Accounts {
        Accounts::new(Site::accounts())
    }

    fn reply(status: u16, body: &str) -> Response {
        Response { status, body: body.into() }
    }

    #[test]
    fn sign_in_posts_the_credentials_to_the_account_service_only() {
        let r = accounts().sign_in(" me@example.com ", "pa ss").unwrap();
        assert_eq!((r.method, r.url.as_str()), ("POST", "https://cloud.stitchfoundry.app/v1/signin"));
        assert_eq!(serde_json::from_str::<serde_json::Value>(&r.body).unwrap(), json!({ "email": "me@example.com", "password": "pa ss" }));
        assert!(!r.headers.iter().any(|(k, _)| *k == "Authorization"));
        assert_eq!(accounts().sign_up("me@example.com", "x").unwrap().url, "https://cloud.stitchfoundry.app/v1/signup");
        for (email, password) in [("", "x"), ("nobody", "x"), ("a@b\n.c", "x"), ("me@example.com", ""), ("me@example.com", &"p".repeat(2000))] {
            assert!(accounts().sign_in(email, password).is_err(), "{email:?}");
        }
    }

    #[test]
    fn signed_requests_carry_only_a_session_token() {
        let r = accounts().account("sfs_tok").unwrap();
        assert_eq!(r.url, "https://cloud.stitchfoundry.app/v1/account");
        assert!(r.headers.contains(&("Authorization", "Bearer sfs_tok".into())));
        assert_eq!(accounts().sign_out("sfs_tok").unwrap().url, "https://cloud.stitchfoundry.app/v1/signout");
        assert!(accounts().account("sk-ant-secret").is_err() && accounts().sign_out("").is_err());
    }

    #[test]
    fn a_session_is_read_from_the_answer() {
        let s = accounts().session(&reply(200, r#"{"token":"sfs_abc","account":{"email":"me@example.com","credits":100}}"#)).unwrap();
        assert_eq!(s, Session { token: "sfs_abc".into(), email: "me@example.com".into(), credits: Some(100) });
        let read = accounts().read("sfs_abc", &reply(200, r#"{"email":"me@example.com","credits":97,"packs":[]}"#)).unwrap();
        assert_eq!((read.email.as_str(), read.credits), ("me@example.com", Some(97)));
    }

    #[test]
    fn a_bad_answer_is_an_error_and_never_a_session() {
        let a = accounts();
        for body in ["", "x", "{}", r#"{"token":7}"#, r#"{"token":"sk-ant-1"}"#, r#"{"token":"sfs_"}"#, r#"{"token":"sfs_a b"}"#] {
            assert!(a.session(&reply(200, body)).is_err(), "{body}");
        }
        assert_eq!(a.session(&reply(401, r#"{"error":{"message":"Wrong email or password."}}"#)).unwrap_err(), "Wrong email or password.");
        assert!(a.session(&reply(500, "")).is_err() && a.read("sfs_a", &reply(401, "")).unwrap_err().contains("Sign in again"));
    }
}

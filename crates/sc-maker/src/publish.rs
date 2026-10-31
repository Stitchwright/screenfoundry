//! Publish a design to MakerFoundry as a private draft (`POST /api/v1/designs`). MakerFoundry keeps
//! it as a draft and answers with its page; the person finishes it there (title, licence, price) and
//! publishes it there. Only a Foundry Account session token (`sfs_...`) is ever sent.

use serde_json::json;

use crate::text;
use crate::{Request, Response, Site};

/// The most MakerFoundry takes for a laser (vector) file, before base64: 8 MB (its `docs/api.md`).
pub const MAX_FILE_BYTES: usize = 8 * 1024 * 1024;
/// The longest file name sent.
const MAX_NAME: usize = 120;

/// A draft MakerFoundry kept.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Draft {
    pub id: u64,
    /// MakerFoundry's page for the draft, on the site the request went to.
    pub url: String,
}

/// A design on its way to MakerFoundry.
#[derive(Clone, Debug)]
pub struct Publish {
    site: Site,
    file_name: String,
    file_base64: String,
    app: String,
}

impl Publish {
    /// The SVG `svg_name` (its bytes already base64, standard alphabet with padding) from `app`
    /// (`ScreenFoundry 0.1.0`), for `site`. The name is cleaned to a plain `.svg` file name.
    pub fn new(site: Site, svg_name: &str, file_base64: String, app: &str) -> Result<Self, String> {
        if file_base64.is_empty() {
            return Err("there is nothing to publish".into());
        }
        let valid = |c: char| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=');
        if file_base64.len() % 4 != 0 || !file_base64.chars().all(valid) {
            return Err("the design isn't base64 text".into());
        }
        if file_base64.len() / 4 * 3 > MAX_FILE_BYTES + 3 {
            return Err(format!("the design is bigger than MakerFoundry takes ({} MB)", MAX_FILE_BYTES / 1024 / 1024));
        }
        Ok(Self { site, file_name: svg_file_name(svg_name), file_base64, app: text::sentence(app).chars().take(60).collect() })
    }

    /// The site the design goes to.
    pub fn site(&self) -> &Site {
        &self.site
    }

    /// The file name MakerFoundry will see.
    pub fn file_name(&self) -> &str {
        &self.file_name
    }

    /// The request, signed in with `token`. Only a Foundry session token is accepted: anything else
    /// (an API key of another service, a password) is refused and never put in a request.
    pub fn request(&self, token: &str) -> Result<Request, String> {
        let token = session_token(token)?;
        let body = json!({ "file_name": self.file_name, "file": self.file_base64, "app": self.app });
        Ok(Request {
            method: "POST",
            url: self.site.url("/api/v1/designs"),
            headers: vec![("Authorization", format!("Bearer {token}")), ("Content-Type", "application/json".into())],
            body: body.to_string(),
        })
    }

    /// Read MakerFoundry's answer: the draft (`201`), else its sentence for the person. The draft's
    /// page is returned only when it is on the same site the request went to.
    pub fn answer(&self, response: &Response) -> Result<Draft, String> {
        if response.status != 201 {
            return Err(refusal(response));
        }
        let v: serde_json::Value = serde_json::from_str(&response.body).map_err(|_| "MakerFoundry's answer wasn't readable".to_string())?;
        let id = v.get("id").and_then(serde_json::Value::as_u64).ok_or("MakerFoundry's answer had no design number")?;
        let url = v.get("url").and_then(serde_json::Value::as_str).ok_or("MakerFoundry's answer had no page for the design")?;
        if !self.site.owns(url) {
            return Err("MakerFoundry answered with a page on another site, so it was not opened".into());
        }
        Ok(Draft { id, url: url.to_string() })
    }
}

/// `token` when it is a Foundry Account session token: `sfs_` and then letters, digits, `-` and `_`.
pub fn session_token(token: &str) -> Result<&str, String> {
    let rest = token.strip_prefix("sfs_").unwrap_or_default();
    if rest.is_empty() || rest.len() > 200 || !rest.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err("sign in to your Foundry Account first".into());
    }
    Ok(token)
}

/// The sentence for a refused request: MakerFoundry's own when it sent one, else one per status.
pub(crate) fn refusal(response: &Response) -> String {
    if let Some(message) = text::error_message(&response.body) {
        return message;
    }
    match response.status {
        401 => "Your Foundry Account sign-in has ended. Sign in again.".into(),
        403 => "MakerFoundry doesn't allow that.".into(),
        404 => "That design isn't on MakerFoundry.".into(),
        413 => "The design is bigger than MakerFoundry takes.".into(),
        422 => "MakerFoundry couldn't read the design.".into(),
        429 => "MakerFoundry has had too many requests. Try again later.".into(),
        503 => "MakerFoundry can't be reached right now. Try again later.".into(),
        s => format!("MakerFoundry answered with an error ({s})."),
    }
}

/// `name` as a plain `.svg` file name: its last part only, without hidden characters or leading dots,
/// `design.svg` when nothing is left.
pub fn svg_file_name(name: &str) -> String {
    let last = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let clean: String = last.chars().filter(|c| !text::hidden(*c)).collect();
    let clean = clean.trim();
    let stem = match clean.len().checked_sub(4).and_then(|at| clean.get(at..)) {
        Some(ext) if ext.eq_ignore_ascii_case(".svg") => &clean[..clean.len() - 4],
        _ => clean,
    };
    let stem: String = stem.trim().trim_start_matches('.').chars().take(MAX_NAME).collect();
    let stem = stem.trim().trim_end_matches('.').trim_end();
    if stem.is_empty() { "design.svg".into() } else { format!("{stem}.svg") }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn publish() -> Publish {
        Publish::new(Site::makerfoundry(), "Rose.svg", "PHN2Zy8+".into(), "ScreenFoundry 0.1.0").unwrap()
    }

    fn reply(status: u16, body: &str) -> Response {
        Response { status, body: body.into() }
    }

    #[test]
    fn the_request_is_a_signed_post_of_the_file() {
        let r = publish().request("sfs_abc-DEF_123").unwrap();
        assert_eq!((r.method, r.url.as_str()), ("POST", "https://makerfoundry.app/api/v1/designs"));
        assert!(r.headers.contains(&("Authorization", "Bearer sfs_abc-DEF_123".into())));
        let body: serde_json::Value = serde_json::from_str(&r.body).unwrap();
        assert_eq!(body, json!({ "file_name": "Rose.svg", "file": "PHN2Zy8+", "app": "ScreenFoundry 0.1.0" }));
    }

    #[test]
    fn only_a_foundry_session_token_is_ever_sent() {
        let p = publish();
        for bad in ["", "sfs_", "sk-ant-api03-secret", "Bearer sfs_x", "sfs_a b", "sfs_a\r\nX: y", "sfs_é", "SFS_abc", " sfs_abc", &format!("sfs_{}", "a".repeat(300))] {
            assert!(p.request(bad).is_err(), "{bad:?}");
        }
        assert!(p.request("sfs_ok").is_ok());
    }

    #[test]
    fn nothing_empty_or_oversized_is_sent() {
        let s = Site::makerfoundry;
        assert!(Publish::new(s(), "a.svg", String::new(), "x").is_err());
        assert!(Publish::new(s(), "a.svg", "not base64!".into(), "x").is_err());
        assert!(Publish::new(s(), "a.svg", "abc".into(), "x").is_err());
        assert!(Publish::new(s(), "a.svg", "A".repeat(4 * (MAX_FILE_BYTES / 3 + 10)), "x").is_err());
        assert!(Publish::new(s(), "a.svg", "A".repeat(4 * 1000), "x").is_ok());
    }

    #[test]
    fn file_names_are_plain_svg_names() {
        for (given, want) in [
            ("Rose.svg", "Rose.svg"),
            ("Rose", "Rose.svg"),
            ("Rose.SVG", "Rose.svg"),
            ("art.vectorcraft", "art.vectorcraft.svg"),
            ("", "design.svg"),
            (".svg", "design.svg"),
            ("../../etc/passwd", "passwd.svg"),
            ("C:\\Users\\me\\shirt.svg", "shirt.svg"),
            ("..hidden.svg", "hidden.svg"),
            ("a\r\nb.svg", "ab.svg"),
            ("gnp\u{202E}.svg", "gnp.svg"),
            ("nul\0.svg", "nul.svg"),
            ("dots...svg", "dots.svg"),
            ("trailing. .svg", "trailing.svg"),
        ] {
            assert_eq!(svg_file_name(given), want, "{given:?}");
        }
        assert!(svg_file_name(&"x".repeat(500)).len() <= MAX_NAME + 4);
    }

    #[test]
    fn a_draft_is_read_from_a_201() {
        let d = publish().answer(&reply(201, r#"{"id": 42, "url": "https://makerfoundry.app/designs/42", "kind": "laser", "format": "SVG"}"#)).unwrap();
        assert_eq!(d, Draft { id: 42, url: "https://makerfoundry.app/designs/42".into() });
    }

    #[test]
    fn a_draft_page_on_another_site_is_never_returned() {
        for url in [
            "https://evil.example/designs/42",
            "https://makerfoundry.app.evil.example/designs/42",
            "https://makerfoundry.app@evil.example/designs/42",
            "https://makerfoundry.app:444/designs/42",
            "http://makerfoundry.app/designs/42",
            "javascript:alert(1)",
            "//evil.example/designs/42",
            "/designs/42",
            "https://www.makerfoundry.app/designs/42",
        ] {
            let body = json!({ "id": 42, "url": url }).to_string();
            let e = publish().answer(&reply(201, &body)).unwrap_err();
            assert!(e.contains("another site"), "{url}: {e}");
        }
    }

    #[test]
    fn an_unreadable_201_is_an_error_not_a_draft() {
        for body in ["", "no", "{}", r#"{"id":"42","url":"https://makerfoundry.app/d"}"#, r#"{"id":42}"#, r#"{"id":42,"url":7}"#, r#"{"id":-1,"url":"https://makerfoundry.app/d"}"#] {
            assert!(publish().answer(&reply(201, body)).is_err(), "{body}");
        }
    }

    #[test]
    fn a_refusal_shows_makerfoundrys_own_sentence() {
        let p = publish();
        let e = p.answer(&reply(429, r#"{"error":{"message":"Too many designs today."}}"#)).unwrap_err();
        assert_eq!(e, "Too many designs today.");
        assert!(p.answer(&reply(401, "")).unwrap_err().contains("Sign in again"));
        assert!(p.answer(&reply(413, "")).unwrap_err().contains("bigger"));
        assert!(p.answer(&reply(422, r#"{"error":{"message":"That SVG has no shapes."}}"#)).unwrap_err().contains("no shapes"));
        assert!(p.answer(&reply(503, "<html>")).unwrap_err().contains("can't be reached"));
        assert!(p.answer(&reply(418, "")).unwrap_err().contains("418"));
        // hostile text is made safe, whatever the status
        let e = p.answer(&reply(422, "{\"error\":{\"message\":\"a\\r\\n\\u202eb\"}}")).unwrap_err();
        assert_eq!(e, "a b");
        // a 200 (or anything but 201) is not a draft
        assert!(p.answer(&reply(200, r#"{"id":1,"url":"https://makerfoundry.app/d/1"}"#)).is_err());
    }
}

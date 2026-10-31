//! Hostile input to MakerFoundry's side: whatever comes in, nothing panics, and everything that is
//! accepted is MakerFoundry's (SC-11).
// Integration tests: unwrapping and panicking on failure is fine here, unlike in shipped code.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use proptest::prelude::*;
use sc_maker::account::Accounts;
use sc_maker::link::{self, MAX_ADDRESS};
use sc_maker::publish::{Publish, svg_file_name};
use sc_maker::{Response, Site};

/// Pieces that make addresses look almost right.
const PIECES: &[&str] = &[
    "https://", "http://", "HTTPS://", "//", "makerfoundry.app", "www.makerfoundry.app", "evil.com", ".", "..", "/", "//", "\\", "@", ":", "443", "#", "?", "&",
    "=", "%", "%2e", "%2f", "%5c", "%00", "%0d%0a", "%25", "%zz", "%e2%80%ae", "api", "v1", "designs", "42", "file", "rose.svg", " ", "\t", "\n", "é", "а", "。",
    "\u{202e}", "\u{0}", "-", "_", "~", "+", "a",
];

fn arb_address() -> impl Strategy<Value = String> {
    prop::collection::vec(prop::sample::select(PIECES), 0..24).prop_map(|v| v.concat())
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 2000, failure_persistence: None, ..ProptestConfig::default() })]

    /// Anything accepted is an `https` address on MakerFoundry's own hosts, under /api/v1/, plain.
    #[test]
    fn an_accepted_address_is_always_makerfoundrys(raw in prop_oneof![arb_address(), any::<String>()]) {
        if let Ok(checked) = link::open_link(&raw) {
            let a = checked.as_str();
            let rest = a.strip_prefix("https://").expect("https");
            let host = rest.split(['/', '?']).next().unwrap();
            prop_assert!(host == "makerfoundry.app" || host == "www.makerfoundry.app", "{a}");
            prop_assert!(rest[host.len()..].starts_with("/api/v1/") && a.len() <= MAX_ADDRESS, "{a}");
            prop_assert!(a.bytes().all(|b| b.is_ascii_graphic()) && !a.contains(['\\', '#', '@']), "{a}");
            // the name is plain, whatever the header says
            let name = link::file_name(&checked, Some(&raw));
            prop_assert!(!name.is_empty() && !name.contains(['/', '\\']) && !name.starts_with('.') && name.chars().count() <= 120 && !name.chars().any(char::is_control), "{name:?}");
        }
    }

    /// Hostile headers and names never panic and always give a plain name.
    #[test]
    fn file_names_are_plain_whatever_the_header(header in any::<String>(), name in any::<String>()) {
        let checked = link::open_link("https://makerfoundry.app/api/v1/designs/1/file/a.svg").unwrap();
        let got = link::file_name(&checked, Some(&header));
        prop_assert!(!got.is_empty() && !got.contains(['/', '\\']) && !got.starts_with('.') && got.chars().count() <= 120 && !got.chars().any(char::is_control), "{got:?}");
        let clean = link::clean_name(&name);
        prop_assert!(!clean.contains(['/', '\\']) && !clean.starts_with('.') && clean.chars().count() <= 120, "{clean:?}");
        let svg = svg_file_name(&name);
        prop_assert!(svg.ends_with(".svg") && !svg.contains(['/', '\\']) && !svg.starts_with('.') && !svg.chars().any(char::is_control), "{svg:?}");
        let _ = link::require_svg(&name);
    }

    /// A draft page from an answer is returned only when it is on the site the request went to.
    #[test]
    fn a_draft_page_is_always_on_the_requests_own_site(url in prop_oneof![arb_address(), any::<String>()], status in 0u16..700) {
        let publish = Publish::new(Site::makerfoundry(), "a.svg", "PHN2Zy8+".into(), "ScreenFoundry").unwrap();
        let body = serde_json::json!({ "id": 1, "url": url }).to_string();
        if let Ok(draft) = publish.answer(&Response { status, body }) {
            prop_assert!(status == 201 && draft.url.starts_with("https://makerfoundry.app") && Site::makerfoundry().owns(&draft.url), "{}", draft.url);
        }
    }

    /// Text from a service is made safe whatever it holds, and nothing panics.
    #[test]
    fn answers_never_panic_and_show_no_hidden_characters(status in 0u16..700, body in any::<String>()) {
        let publish = Publish::new(Site::makerfoundry(), "a.svg", "PHN2Zy8+".into(), "x").unwrap();
        if let Err(e) = publish.answer(&Response { status, body: body.clone() }) {
            prop_assert!(!e.chars().any(char::is_control) && e.chars().count() <= 300, "{e:?}");
        }
        let accounts = Accounts::new(Site::accounts());
        let _ = accounts.session(&Response { status, body: body.clone() });
        let _ = accounts.read("sfs_x", &Response { status, body: body.clone() });
        let e = link::refusal(status, &body);
        prop_assert!(!e.chars().any(char::is_control) && e.chars().count() <= 300, "{e:?}");
    }

    /// Only a well-formed session token is ever put in a request; nothing else is sent.
    #[test]
    fn only_session_tokens_are_signed_with(token in prop_oneof![any::<String>(), "[ -~]{0,40}", "sfs_[ -~]{0,40}"]) {
        let publish = Publish::new(Site::makerfoundry(), "a.svg", "PHN2Zy8+".into(), "x").unwrap();
        for request in [publish.request(&token), Accounts::new(Site::accounts()).account(&token), Accounts::new(Site::accounts()).sign_out(&token)] {
            if let Ok(r) = request {
                let auth = r.headers.iter().find(|(k, _)| *k == "Authorization").map(|(_, v)| v.clone()).unwrap();
                prop_assert_eq!(auth, format!("Bearer {token}"));
                prop_assert!(token.starts_with("sfs_") && token.len() > 4 && token.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'));
            }
        }
    }

    /// Sites parse to bare origins or not at all.
    #[test]
    fn a_site_is_a_bare_origin_or_nothing(raw in prop_oneof![arb_address(), any::<String>()]) {
        if let Ok(site) = Site::parse(&raw) {
            prop_assert!(!site.origin()[8..].contains(['/', '?', '#', '@', '\\', ' ']), "{}", site.origin());
            prop_assert!(site.origin().starts_with("https://") || site.origin().starts_with("http://127.0.0.1") || site.origin().starts_with("http://localhost"));
        }
    }
}

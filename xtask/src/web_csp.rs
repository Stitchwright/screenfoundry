//! `cargo xtask web-csp [index.html] [--check]`: the web app's content security policy (SC-11).
//!
//! `apps/vectorcraft-web/index.html` carries the policy with `__SCRIPT_HASHES__` where the hashes
//! of the page's inline scripts go: Trunk writes an inline module script whose text changes with
//! every build (it names the hashed `.js` and `.wasm` files), so no hash can be written by hand.
//! As the Trunk `post_build` hook this fills the placeholder in the built page with the SHA-256 of
//! every inline script, then checks the page. With `--check` it only checks (CI runs it on the
//! built site): a page whose script isn't in `script-src` loads nothing and shows a blank window,
//! which is how LaserFoundry's hosted app was blank for 12 days after a rename.
//!
//! The check also holds the policy to what the page needs and no more: no `unsafe-inline` or
//! `unsafe-eval` for scripts, MakerFoundry's two hosts and the Foundry Account service in
//! `connect-src`, no plug-ins.

use std::path::{Path, PathBuf};

use crate::sha256;

/// Where the hashes go in the policy of the source page.
const PLACEHOLDER: &str = "__SCRIPT_HASHES__";
/// What `connect-src` has to name, for Publish to MakerFoundry, Open in ScreenFoundry and sign-in.
const CONNECT: [&str; 3] = ["https://makerfoundry.app", "https://www.makerfoundry.app", "https://cloud.stitchfoundry.app"];

/// The command: the page is the first argument, else Trunk's staging folder's, else `dist/web`.
pub fn run(root: &Path, args: &[&str]) -> Result<(), String> {
    let only_check = args.contains(&"--check");
    let given = args.iter().find(|a| !a.starts_with("--"));
    let staging = std::env::var_os("TRUNK_STAGING_DIR").filter(|v| !v.is_empty()).map(|d| PathBuf::from(d).join("index.html"));
    let page = match (given, staging) {
        (Some(p), _) => root.join(p),
        (None, Some(s)) => s,
        (None, None) => root.join("dist/web/index.html"),
    };
    let html = std::fs::read_to_string(&page).map_err(|e| format!("{}: {e}", page.display()))?;
    let html = if only_check || !html.contains(PLACEHOLDER) {
        html
    } else {
        let filled = fill(&html)?;
        std::fs::write(&page, &filled).map_err(|e| format!("{}: {e}", page.display()))?;
        println!("web-csp: {} script hash(es) written into {}", inline_scripts(&filled).len(), page.display());
        filled
    };
    check(&html)?;
    println!("web-csp: {} is consistent: {} inline script(s) allowed by script-src", page.display(), inline_scripts(&html).len());
    Ok(())
}

/// The text of every inline `<script>` (one without `src`), as the browser hashes it.
fn inline_scripts(html: &str) -> Vec<&str> {
    let mut found = vec![];
    let mut rest = html;
    while let Some(at) = rest.find("<script") {
        rest = &rest[at + "<script".len()..];
        let Some(open_end) = rest.find('>') else { break };
        let (attributes, after) = (&rest[..open_end], &rest[open_end + 1..]);
        let Some(close) = after.find("</script>") else { break };
        if !attributes.split_whitespace().any(|a| a == "src" || a.starts_with("src=")) {
            found.push(&after[..close]);
        }
        rest = &after[close..];
    }
    found
}

/// The policy in the page's `<meta http-equiv="Content-Security-Policy" content="...">`.
fn policy(html: &str) -> Option<&str> {
    let at = html.find("Content-Security-Policy")?;
    let tag_start = html[..at].rfind("<meta")?;
    let tag = &html[tag_start..tag_start + html[tag_start..].find('>')?];
    let content = &tag[tag.find("content=\"")? + "content=\"".len()..];
    Some(&content[..content.find('"')?])
}

/// The `'sha256-...'` source for `script`.
fn source_of(script: &str) -> String {
    let digest: Vec<u8> = sha256::hex(script.as_bytes()).as_bytes().chunks(2).filter_map(|p| u8::from_str_radix(std::str::from_utf8(p).ok()?, 16).ok()).collect();
    format!("'sha256-{}'", base64(&digest))
}

/// Standard base64 with padding.
fn base64(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[(n >> (18 - 6 * i) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// `html` with the placeholder replaced by the hash of each inline script.
fn fill(html: &str) -> Result<String, String> {
    if !html.contains(PLACEHOLDER) {
        return Err(format!("the page has no {PLACEHOLDER} in its content security policy"));
    }
    let sources: Vec<String> = inline_scripts(html).into_iter().map(source_of).collect();
    Ok(html.replace(PLACEHOLDER, &sources.join(" ")))
}

/// Whether the page's policy lets its inline scripts run and asks no more than it needs.
fn check(html: &str) -> Result<(), String> {
    let policy = policy(html).ok_or("the page has no content security policy")?;
    if policy.contains(PLACEHOLDER) {
        return Err(format!("the policy still holds {PLACEHOLDER}: run `cargo xtask web-csp` on the built page"));
    }
    let directive = |name: &str| policy.split(';').map(str::trim).find(|d| d.split_whitespace().next() == Some(name));
    let script = directive("script-src").ok_or("the policy has no script-src")?;
    let sources: Vec<&str> = script.split_whitespace().skip(1).collect();
    for banned in ["'unsafe-inline'", "'unsafe-eval'", "*", "https:", "http:", "data:", "blob:"] {
        if sources.contains(&banned) {
            return Err(format!("script-src allows {banned}"));
        }
    }
    for body in inline_scripts(html) {
        let want = source_of(body);
        if !sources.contains(&want.as_str()) {
            return Err(format!("an inline script's hash {want} is not in script-src: the page would load nothing (script starts: {:?})", body.trim().chars().take(60).collect::<String>()));
        }
    }
    let connect = directive("connect-src").ok_or("the policy has no connect-src")?;
    for site in CONNECT {
        if !connect.split_whitespace().any(|s| s == site) {
            return Err(format!("connect-src does not name {site}"));
        }
    }
    if directive("object-src").map(str::trim) != Some("object-src 'none'") {
        return Err("the policy does not say object-src 'none'".into());
    }
    if directive("default-src").is_none() {
        return Err("the policy has no default-src".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = include_str!("../../apps/vectorcraft-web/index.html");

    /// A page as Trunk writes it: the policy of the source, an inline module script and a linked one.
    fn built(script: &str) -> String {
        let page = SOURCE.replace("</body>", &format!("<script type=\"module\">{script}</script><script src=\"x.js\"></script></body>"));
        fill(&page).unwrap()
    }

    const SCRIPT: &str = "import init from './vectorcraft-web-abc.js'; init('./vectorcraft-web-abc_bg.wasm');";

    #[test]
    fn the_source_page_has_a_policy_with_the_placeholder_and_what_the_app_needs() {
        assert!(policy(SOURCE).unwrap().contains(PLACEHOLDER));
        let p = policy(&built(SCRIPT)).unwrap().to_string();
        for site in CONNECT {
            assert!(p.contains(site), "{site}");
        }
        assert!(p.contains("'wasm-unsafe-eval'") && !p.contains("'unsafe-eval'"));
        assert!(check(&built(SCRIPT)).is_ok());
    }

    #[test]
    fn the_hash_matches_the_scripts_sha256() {
        // the empty script's SHA-256 in base64 is a well-known value
        assert_eq!(source_of(""), "'sha256-47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU='");
        let page = built(SCRIPT);
        assert!(policy(&page).unwrap().contains(&source_of(SCRIPT)));
    }

    #[test]
    fn base64_matches_rfc_4648_vectors() {
        for (plain, coded) in [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foob", "Zm9vYg=="), ("fooba", "Zm9vYmE="), ("foobar", "Zm9vYmFy")] {
            assert_eq!(base64(plain.as_bytes()), coded);
        }
    }

    #[test]
    fn a_script_that_changed_after_the_policy_was_written_is_caught() {
        // the stale hash that left LaserFoundry's page blank: the script changed and the hash didn't
        let page = built(SCRIPT).replace("abc", "def");
        let e = check(&page).unwrap_err();
        assert!(e.contains("not in script-src"), "{e}");
    }

    #[test]
    fn every_inline_script_needs_its_hash_and_linked_ones_do_not() {
        let two = SOURCE.replace("</body>", "<script>a()</script><script type=\"module\">b()</script><script src=\"c.js\" defer></script></body>");
        let filled = fill(&two).unwrap();
        assert_eq!(inline_scripts(&filled), ["a()", "b()"]);
        assert!(check(&filled).is_ok());
        let missing = filled.replace(&source_of("b()"), "");
        assert!(check(&missing).is_err());
    }

    #[test]
    fn an_unfilled_or_loose_policy_is_refused() {
        assert!(check(SOURCE).unwrap_err().contains(PLACEHOLDER));
        assert!(check("<html></html>").is_err());
        let good = built(SCRIPT);
        for (from, to) in [
            ("script-src 'self'", "script-src 'self' 'unsafe-inline'"),
            ("script-src 'self'", "script-src 'self' 'unsafe-eval'"),
            ("script-src 'self'", "script-src 'self' https:"),
            ("script-src 'self'", "script-src 'self' *"),
            ("https://makerfoundry.app", "https://example.com"),
            ("https://www.makerfoundry.app", ""),
            ("https://cloud.stitchfoundry.app", ""),
            ("object-src 'none'", "object-src 'self'"),
        ] {
            assert!(check(&good.replace(from, to)).is_err(), "{from} -> {to}");
        }
    }

    #[test]
    fn filling_needs_the_placeholder() {
        assert!(fill("<html></html>").is_err());
    }
}

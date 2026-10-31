//! Open in ScreenFoundry: a MakerFoundry design page links to the web app as `?open=<the design
//! file's address>`. The address came from outside, so it is checked strictly before anything is
//! fetched, and the page fetches the *checked* address ([`Checked::as_str`]), never the text that
//! came in.

use crate::text;

/// The longest `?open=` address accepted (characters).
pub const MAX_ADDRESS: usize = 2048;
/// The most bytes fetched for one design (32 MB).
pub const MAX_BYTES: usize = 32 * 1024 * 1024;
/// How long the fetch may take before it is given up (seconds).
pub const TIMEOUT_SECONDS: u32 = 30;
/// The longest file name kept.
const MAX_NAME: usize = 120;
/// The only sites a design opens from, and the only place on them.
const HOSTS: [&str; 2] = ["makerfoundry.app", "www.makerfoundry.app"];
const PATH_START: &str = "/api/v1/";

/// A design address that passed [`open_link`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Checked {
    address: String,
}

impl Checked {
    /// The address to fetch: `https`, MakerFoundry's host in lower case, a checked path and query.
    pub fn as_str(&self) -> &str {
        &self.address
    }

    /// The last part of the path, percent-decoded and cleaned: the name the file opens under when
    /// the answer names none.
    pub fn file_name(&self) -> String {
        let path = self.address.split('?').next().unwrap_or_default();
        clean_name(&decode(path.rsplit('/').next().unwrap_or_default()).unwrap_or_default())
    }
}

/// The address in `?open=` (as the page read it, already URL-decoded once) when it may be fetched:
/// `https` only, the host exactly `makerfoundry.app` or `www.makerfoundry.app` (no user name, port
/// or trailing dot), a path under `/api/v1/` whose parts decode to plain names, a query of plain
/// characters, no fragment; no spaces, control characters, backslashes or non-ASCII; at most
/// [`MAX_ADDRESS`] characters. Else a sentence saying why not.
pub fn open_link(raw: &str) -> Result<Checked, &'static str> {
    const INVALID: &str = "the link to the design isn't a valid address";
    if raw.len() > MAX_ADDRESS || !raw.bytes().all(|b| b.is_ascii_graphic()) || raw.contains('\\') {
        return Err(INVALID);
    }
    let scheme_len = "https://".len();
    let rest = match raw.get(..scheme_len) {
        Some(s) if s.eq_ignore_ascii_case("https://") => &raw[scheme_len..],
        _ => return Err("designs open only from https:// addresses"),
    };
    if rest.contains('#') {
        return Err(INVALID);
    }
    let end = rest.find(['/', '?']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(end);
    let host = authority.to_ascii_lowercase();
    if !HOSTS.contains(&host.as_str()) {
        return Err("designs open only from MakerFoundry");
    }
    let (path, query) = match tail.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (tail, None),
    };
    if !path.starts_with(PATH_START) || path.len() == PATH_START.len() {
        return Err("designs open only from MakerFoundry's design files");
    }
    for segment in path[1..].split('/') {
        plain_segment(segment).ok_or(INVALID)?;
    }
    if let Some(query) = query
        && !plain_query(query)
    {
        return Err(INVALID);
    }
    Ok(Checked { address: format!("https://{host}{path}{}", query.map(|q| format!("?{q}")).unwrap_or_default()) })
}

/// Whether `bytes` fit the most a design may be.
pub fn fits(bytes: u64) -> bool {
    bytes <= MAX_BYTES as u64
}

/// The name the file opens under: the `filename*` or `filename` of a `Content-Disposition` header,
/// else the end of the address; cleaned (see [`clean_name`]), `design` when nothing is left.
pub fn file_name(checked: &Checked, disposition: Option<&str>) -> String {
    let from_header = disposition.and_then(header_name).map(|n| clean_name(&n)).filter(|n| !n.is_empty());
    let name = from_header.unwrap_or_else(|| checked.file_name());
    if name.is_empty() { "design".into() } else { name }
}

/// `name` as a plain file name: its last part only (no `/` or `\`), without control or hidden
/// characters, without leading dots, at most 120 characters.
pub fn clean_name(name: &str) -> String {
    let last = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let kept: String = last.chars().filter(|c| !text::hidden(*c)).collect();
    kept.trim().trim_start_matches('.').chars().take(MAX_NAME).collect::<String>().trim().to_string()
}

/// Whether the file is an SVG by its name: ScreenFoundry opens only SVG designs from MakerFoundry.
pub fn require_svg(name: &str) -> Result<(), String> {
    match name.rsplit_once('.') {
        Some((stem, ext)) if ext.eq_ignore_ascii_case("svg") && !stem.is_empty() => Ok(()),
        _ => Err(format!("ScreenFoundry opens SVG designs from MakerFoundry; “{name}” isn't one")),
    }
}

/// The sentence for an answer that isn't the file: MakerFoundry's own (`403` for a design for sale,
/// `404`, `429`...), else one per status.
pub fn refusal(status: u16, body: &str) -> String {
    crate::publish::refusal(&crate::Response { status, body: body.to_string() })
}

/// A path part that decodes to a plain name: not empty, not `.` or `..`, and nothing that is, or
/// could be (another layer of `%XX`), a separator, a control or a hidden character.
fn plain_segment(segment: &str) -> Option<()> {
    let plain = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '~' | '%' | '+' | '(' | ')' | '!' | '\'' | ',' | ';' | '=');
    if segment.is_empty() || !segment.chars().all(plain) {
        return None;
    }
    let decoded = decode(segment)?;
    let layered =
        decoded.char_indices().any(|(i, c)| c == '%' && decoded.get(i + 1..i + 3).is_some_and(|h| h.bytes().all(|b| b.is_ascii_hexdigit())));
    let bad = decoded == "." || decoded == ".." || layered || decoded.chars().any(|c| c == '/' || c == '\\' || text::hidden(c));
    (!bad).then_some(())
}

/// Whether `query` is plain: letters, digits, `- . _ ~ + = &` and well-formed `%XX`.
fn plain_query(query: &str) -> bool {
    query.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '~' | '%' | '+' | '=' | '&'))
        && decode(query).is_some_and(|d| !d.chars().any(text::hidden))
}

/// `s` with `%XX` escapes decoded, when every escape is well formed and the result is UTF-8.
fn decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while let Some(&c) = b.get(i) {
        if c == b'%' {
            let hex = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
            if !hex.bytes().all(|h| h.is_ascii_hexdigit()) {
                return None;
            }
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(c);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// The file name a `Content-Disposition` header gives: `filename*=UTF-8''...` first, else `filename=`.
/// Only the first 1,024 characters of the header are read.
fn header_name(header: &str) -> Option<String> {
    let header: String = header.chars().take(1024).collect();
    let mut plain = None;
    for part in header.split(';').skip(1) {
        let Some((key, value)) = part.split_once('=') else { continue };
        let value = value.trim();
        match key.trim().to_ascii_lowercase().as_str() {
            "filename*" => {
                let encoded = value.strip_prefix("UTF-8''").or_else(|| value.strip_prefix("utf-8''"));
                if let Some(name) = encoded.and_then(decode) {
                    return Some(name);
                }
            }
            "filename" if plain.is_none() => plain = Some(value.trim_matches('"').to_string()),
            _ => {}
        }
    }
    plain
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = "https://makerfoundry.app/api/v1/designs/42/file/rose.svg";

    #[test]
    fn a_makerfoundry_design_file_is_accepted_and_rebuilt() {
        assert_eq!(open_link(GOOD).unwrap().as_str(), GOOD);
        assert_eq!(
            open_link("https://www.makerfoundry.app/api/v1/designs/42/file/rose.svg").unwrap().as_str(),
            "https://www.makerfoundry.app/api/v1/designs/42/file/rose.svg"
        );
        // the address fetched is the rebuilt one: scheme and host in lower case
        assert_eq!(open_link("HTTPS://MakerFoundry.APP/api/v1/designs/42/file/rose.svg").unwrap().as_str(), GOOD);
        assert_eq!(open_link("https://makerfoundry.app/api/v1/designs/7/file/Big%20Rose%20%281%29.svg").unwrap().file_name(), "Big Rose (1).svg");
        // a buyer's link
        assert!(open_link("https://makerfoundry.app/api/v1/designs/42/file/rose.svg?t=aB3-_x.y~z%2B").is_ok());
    }

    #[test]
    fn look_alike_hosts_and_schemes_are_refused() {
        for bad in [
            "http://makerfoundry.app/api/v1/designs/1/file/a.svg",
            "ftp://makerfoundry.app/api/v1/designs/1/file/a.svg",
            "javascript:alert(1)",
            "data:image/svg+xml;base64,PHN2Zz4=",
            "//makerfoundry.app/api/v1/designs/1/file/a.svg",
            "makerfoundry.app/api/v1/designs/1/file/a.svg",
            "https:makerfoundry.app/api/v1/designs/1/file/a.svg",
            "https:///makerfoundry.app/api/v1/designs/1/file/a.svg",
            "https://makerfoundry.app.evil.com/api/v1/designs/1/file/a.svg",
            "https://makerfoundry.app.evil.com./api/v1/designs/1/file/a.svg",
            "https://evilmakerfoundry.app/api/v1/designs/1/file/a.svg",
            "https://makerfoundry.app@evil.com/api/v1/designs/1/file/a.svg",
            "https://evil.com@makerfoundry.app/api/v1/designs/1/file/a.svg",
            "https://user:pw@makerfoundry.app/api/v1/designs/1/file/a.svg",
            "https://makerfoundry.app:443/api/v1/designs/1/file/a.svg",
            "https://makerfoundry.app:8443/api/v1/designs/1/file/a.svg",
            "https://makerfoundry.app./api/v1/designs/1/file/a.svg",
            "https://sub.makerfoundry.app/api/v1/designs/1/file/a.svg",
            "https://evil.com/makerfoundry.app/api/v1/designs/1/file/a.svg",
            "https://evil.com?makerfoundry.app/api/v1/designs/1/file/a.svg",
            "https://evil.com#@makerfoundry.app/api/v1/designs/1/file/a.svg",
            "https://makerfoundry.app\\@evil.com/api/v1/designs/1/file/a.svg",
            "https://makerfoundry.app%2eevil.com/api/v1/designs/1/file/a.svg",
            "https://makerfoundry.app%40evil.com/api/v1/designs/1/file/a.svg",
            "https://makerfoundry.app%2f@evil.com/api/v1/designs/1/file/a.svg",
            "https://[::1]/api/v1/designs/1/file/a.svg",
            "https://127.0.0.1/api/v1/designs/1/file/a.svg",
            "https://makerfoundry.app",
            "https://",
            "",
        ] {
            assert!(open_link(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn unicode_look_alikes_are_refused() {
        for bad in [
            "https://makerfoundry.аpp/api/v1/designs/1/file/a.svg",
            "https://makerfoundry.app/api/v1/designs/1/file/а.svg",
            "https://makerfоundry.app/api/v1/designs/1/file/a.svg",
            "https://makerfoundry。app/api/v1/designs/1/file/a.svg",
            "https://makerfoundry．app/api/v1/designs/1/file/a.svg",
            "https://ｍakerfoundry.app/api/v1/designs/1/file/a.svg",
            "https://makerfoundry.app/api/v1/designs/1/file/a\u{202e}gvs.svg",
            "https://makerfoundry.app/api/v1/designs/1/file/a\u{200b}.svg",
            "https://xn--makerfoundry-9ta.app/api/v1/designs/1/file/a.svg",
        ] {
            assert!(open_link(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn the_path_must_stay_under_the_design_files() {
        for bad in [
            "https://makerfoundry.app/",
            "https://makerfoundry.app/api/v1",
            "https://makerfoundry.app/api/v1/",
            "https://makerfoundry.app/api/v2/designs/1/file/a.svg",
            "https://makerfoundry.app/API/V1/designs/1/file/a.svg",
            "https://makerfoundry.app/designs/1/file/a.svg",
            "https://makerfoundry.app/api/v1/../admin/a.svg",
            "https://makerfoundry.app/api/v1/designs/../../../admin/a.svg",
            "https://makerfoundry.app/api/v1/designs/./1/file/a.svg",
            "https://makerfoundry.app/api/v1/%2e%2e/admin/a.svg",
            "https://makerfoundry.app/api/v1/%2E%2E/admin/a.svg",
            "https://makerfoundry.app/api/v1/designs/%2e%2e%2fadmin/a.svg",
            "https://makerfoundry.app/api/v1/designs/..%2fadmin/a.svg",
            "https://makerfoundry.app/api/v1/designs/..%5cadmin/a.svg",
            "https://makerfoundry.app/api/v1/designs/%252e%252e/a.svg",
            "https://makerfoundry.app/api/v1/designs/1%2f2/a.svg",
            "https://makerfoundry.app/api/v1/designs/1//file/a.svg",
            "https://makerfoundry.app/api/v1/designs/1/file/",
            "https://makerfoundry.app/api/v1/designs/1/file/a.svg/",
            "https://makerfoundry.app/api/v1/designs/1/file/a%00.svg",
            "https://makerfoundry.app/api/v1/designs/1/file/a%0d%0a.svg",
            "https://makerfoundry.app/api/v1/designs/1/file/a%5cb.svg",
            "https://makerfoundry.app/api/v1/designs/1/file/a%e2%80%aegvs.svg",
            "https://makerfoundry.app/api/v1/designs/1/file/a%ff.svg",
            "https://makerfoundry.app/api/v1/designs/1/file/a%zz.svg",
            "https://makerfoundry.app/api/v1/designs/1/file/a%2.svg",
            "https://makerfoundry.app/api/v1/designs/1/file/a%.svg",
            "https://makerfoundry.app/api/v1/designs/1/file/a:b.svg",
            "https://makerfoundry.app/api/v1/designs/1/file/a;b/../../c.svg",
        ] {
            assert!(open_link(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn spaces_controls_backslashes_and_fragments_are_refused() {
        for bad in [
            "https://makerfoundry.app/api/v1/designs/1/file/a b.svg",
            " https://makerfoundry.app/api/v1/designs/1/file/a.svg",
            "https://makerfoundry.app/api/v1/designs/1/file/a.svg ",
            "https://makerfoundry.app/api/v1/designs/1/file/a.svg\n",
            "https://makerfoundry.app/api/v1/designs/1/file/a.svg\r\nHost: evil.com",
            "https://makerfoundry.app/api/v1/designs/1/file/a\t.svg",
            "https://makerfoundry.app/api/v1/designs/1/file/a\0.svg",
            "https://makerfoundry.app/api/v1/designs/1/file/a\\b.svg",
            "https://makerfoundry.app/api/v1/designs/1/file/a.svg#frag",
            "https://makerfoundry.app/api/v1/designs/1/file/a.svg?x=1#frag",
            "https://makerfoundry.app/api/v1/designs/1/file/a.svg?x=<script>",
            "https://makerfoundry.app/api/v1/designs/1/file/a.svg?x=1?y=2",
            "https://makerfoundry.app/api/v1/designs/1/file/a.svg?x=%zz",
            "https://makerfoundry.app/api/v1/designs/1/file/a.svg?x=%0d%0a",
            "https://makerfoundry.app/api/v1/designs/1/file/a.svg?x=%00",
            "https://makerfoundry.app/api/v1/designs/1/file/a.svg?x=%e2%80%ae",
        ] {
            assert!(open_link(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn the_address_has_a_length_limit() {
        let head = "https://makerfoundry.app/api/v1/designs/1/file/";
        assert!(open_link(&format!("{head}{}.svg", "a".repeat(MAX_ADDRESS - head.len() - 4))).is_ok());
        assert!(open_link(&format!("{head}{}.svg", "a".repeat(MAX_ADDRESS - head.len() - 3))).is_err());
        assert!(open_link(&format!("{head}{}", "a".repeat(100_000))).is_err());
    }

    #[test]
    fn the_size_limit_is_32_mb() {
        assert_eq!(MAX_BYTES, 32 * 1024 * 1024);
        assert!(fits(0) && fits(MAX_BYTES as u64) && !fits(MAX_BYTES as u64 + 1) && !fits(u64::MAX));
    }

    #[test]
    fn file_names_come_from_the_header_or_the_address_and_are_always_plain() {
        let c = open_link("https://makerfoundry.app/api/v1/designs/42/file/Big%20Rose.svg").unwrap();
        assert_eq!(file_name(&c, None), "Big Rose.svg");
        assert_eq!(file_name(&c, Some(r#"attachment; filename="tulip.svg""#)), "tulip.svg");
        assert_eq!(file_name(&c, Some("attachment; filename=tulip.svg")), "tulip.svg");
        assert_eq!(file_name(&c, Some("attachment; filename=\"a.svg\"; filename*=UTF-8''caf%C3%A9.svg")), "café.svg");
        for (header, want) in [
            ("attachment; filename=../../etc/passwd", "passwd"),
            (r#"attachment; filename="..\\..\\win.ini""#, "win.ini"),
            ("attachment; filename=\".hidden.svg\"", "hidden.svg"),
            ("attachment; filename*=UTF-8''%2e%2e%2f%2e%2e%2fshadow", "shadow"),
            ("attachment; filename*=UTF-8''%E2%80%AEgvs.exe", "gvs.exe"),
            ("attachment; filename*=UTF-8''%ff%fe.svg", "Big Rose.svg"),
            ("attachment; filename=\"\"", "Big Rose.svg"),
            ("attachment; filename=\"   \"", "Big Rose.svg"),
            ("attachment", "Big Rose.svg"),
            ("", "Big Rose.svg"),
            ("attachment; filename=\"a\u{0}b.svg\"", "ab.svg"),
            ("attachment; filename=\"a\r\nSet-Cookie: x=y.svg\"", "aSet-Cookie: x=y.svg"),
        ] {
            let got = file_name(&c, Some(header));
            assert_eq!(got, want, "{header:?}");
            assert!(!got.contains(['/', '\\']) && !got.starts_with('.') && !got.is_empty() && got.chars().count() <= MAX_NAME);
        }
        let long = format!("attachment; filename=\"{}.svg\"", "x".repeat(5000));
        assert!(file_name(&c, Some(&long)).chars().count() <= MAX_NAME);
        let huge = format!("attachment; {}; filename=\"late.svg\"", "k=v; ".repeat(500));
        assert_eq!(file_name(&c, Some(&huge)), "Big Rose.svg", "only the start of a header is read");
    }

    #[test]
    fn only_svg_files_open() {
        assert!(require_svg("rose.svg").is_ok() && require_svg("ROSE.SVG").is_ok() && require_svg("a.b.svg").is_ok());
        for bad in ["rose.dxf", "rose.pes", "rose", ".svg", "rose.svg.exe", "rose.svgz", "rose.svg ", "", "rose.png"] {
            assert!(require_svg(bad).is_err(), "{bad:?}");
        }
        assert!(require_svg("rose.dxf").unwrap_err().contains("SVG"));
    }

    #[test]
    fn refusals_use_makerfoundrys_own_sentence() {
        assert_eq!(refusal(403, r#"{"error":{"message":"That design is for sale."}}"#), "That design is for sale.");
        assert_eq!(refusal(404, r#"{"error":{"message":"That design isn't on MakerFoundry."}}"#), "That design isn't on MakerFoundry.");
        assert_eq!(refusal(429, r#"{"error":{"message":"Too many downloads."}}"#), "Too many downloads.");
        assert!(
            refusal(403, "").contains("doesn't allow")
                && refusal(404, "x").contains("isn't on MakerFoundry")
                && refusal(429, "").contains("too many")
        );
    }
}

//! MakerFoundry (SC-11): the drawing as the SVG a draft on MakerFoundry is made from. The engine
//! has no network code: this command exports and checks, and the host (File › Publish to
//! MakerFoundry…, `ui.publishToMakerFoundry`) shows the person what goes where, signs the request
//! with their Foundry Account and sends it (`sc-maker`).

use sc_maker::Site;
use sc_maker::publish::{MAX_FILE_BYTES, svg_file_name};
use serde_json::{Value, json};

use super::fileio::encode_all;
use super::{CommandSpec, EngineError, Result, Session, bad, cmd, has_doc, str_param};

const C: &str = "screen.makerFoundryExport";

pub fn specs() -> Vec<CommandSpec> {
    vec![cmd!(
        query "screen.makerFoundryExport",
        "Export for MakerFoundry",
        [],
        None,
        "{fileName?: the name to publish under (default: the document's title; always a plain .svg name)} the drawing as the SVG MakerFoundry takes as a design (not sent: ui.publishToMakerFoundry sends it) → {fileName, bytes, fileBase64, site, app, warnings}; an empty drawing or one over 8 MB is an error",
        has_doc,
        export
    )]
}

fn export(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let doc = &st.doc;
    if !doc.layers.iter().any(|l| l.children().is_some_and(|c| !c.is_empty())) {
        return Err(bad(C, "the drawing is empty: draw something before publishing it"));
    }
    let name = svg_file_name(str_param(p, "fileName").unwrap_or(&doc.title));
    let mut encoded = encode_all(doc, "svg", &json!({}))?;
    let mut warnings = std::mem::take(&mut encoded.warnings);
    let mut files = encoded.files.into_iter();
    let (_, bytes) = files.next().ok_or_else(|| EngineError::Other("the drawing exported no SVG".into()))?;
    if files.next().is_some() {
        warnings.push("the drawing has several artboards: only the first is published".into());
    }
    if bytes.len() > MAX_FILE_BYTES {
        return Err(bad(C, format!("the SVG is {} MB: MakerFoundry takes at most {} MB", bytes.len().div_ceil(1024 * 1024), MAX_FILE_BYTES / 1024 / 1024)));
    }
    Ok(json!({
        "fileName": name,
        "bytes": bytes.len(),
        "fileBase64": vectorcraft_format::base64_encode(&bytes),
        "site": Site::MAKERFOUNDRY,
        "app": format!("ScreenFoundry {}", env!("CARGO_PKG_VERSION")),
        "warnings": warnings,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_session() -> Session {
        let mut s = Session::new();
        s.execute("file.new", &json!({"width": 200, "height": 100})).unwrap();
        s
    }

    fn session_with_art() -> Session {
        let mut s = empty_session();
        s.execute("shape.rectangle", &json!({"x": 10, "y": 10, "width": 100, "height": 50})).unwrap();
        s
    }

    #[test]
    fn the_drawing_is_exported_as_a_plain_svg_named_for_the_document() {
        let mut s = session_with_art();
        let r = s.execute(C, &json!({})).unwrap();
        assert_eq!((r["site"].as_str(), r["fileName"].as_str()), (Some("https://makerfoundry.app"), Some("Untitled-1.svg")));
        let svg = vectorcraft_format::base64_decode(r["fileBase64"].as_str().unwrap()).unwrap();
        assert_eq!(svg.len() as u64, r["bytes"].as_u64().unwrap());
        assert!(String::from_utf8(svg).unwrap().contains("<svg"));
        assert!(r["app"].as_str().unwrap().starts_with("ScreenFoundry "));
    }

    #[test]
    fn the_file_name_is_always_a_plain_svg_name() {
        let mut s = session_with_art();
        for (given, want) in [("shirt", "shirt.svg"), ("../../etc/passwd", "passwd.svg"), ("a\r\nb.svg", "ab.svg"), ("", "design.svg")] {
            assert_eq!(s.execute(C, &json!({"fileName": given})).unwrap()["fileName"], want, "{given:?}");
        }
    }

    #[test]
    fn an_empty_drawing_is_refused() {
        let mut s = empty_session();
        let e = s.execute(C, &json!({})).unwrap_err().to_string();
        assert!(e.contains("empty"), "{e}");
        assert!(session_with_art().execute(C, &json!({})).is_ok());
        // a document with only an empty layer is empty too
        s.execute("layer.new", &json!({})).unwrap();
        assert!(s.execute(C, &json!({})).is_err());
        // and drawing then deleting everything leaves it empty again
        let mut art = session_with_art();
        art.execute("select.all", &json!({})).unwrap();
        art.execute("edit.clear", &json!({})).unwrap();
        assert!(art.execute(C, &json!({})).is_err());
    }

    #[test]
    fn nothing_open_is_an_error_not_a_crash() {
        let e = Session::new().execute(C, &json!({})).unwrap_err().to_string();
        assert!(e.contains("no document"), "{e}");
    }
}

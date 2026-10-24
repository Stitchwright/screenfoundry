//! Screen Print menu (SC-1): pick a folder, then run a `screen.*` engine command into it.
//!
//! `ui.screenSeparate` writes one film a screen for the artboard (`screen.separate`);
//! `ui.screenHalftone` writes a halftone film (`screen.halftone`); `ui.screenTestPage` writes the
//! calibration films (`screen.testPage`), which need no document. Any engine parameter can be
//! passed along (`colors`, `inks`, `underbase`, `dpi`, ...); `folder` skips the picker.

use serde_json::{Value, json};

use crate::VectorcraftApp;

/// Run `command` with `defaults` under `p`, writing the films into a folder.
fn run(app: &mut VectorcraftApp, p: &Value, command: &str, defaults: Value) -> Result<Value, String> {
    let folder = match p.get("folder").and_then(Value::as_str) {
        Some(f) => f.to_string(),
        None => match crate::picks::folder(app) {
            Some(f) => f,
            None => return Ok(Value::Null),
        },
    };
    let mut params = defaults;
    if let (Some(into), Some(from)) = (params.as_object_mut(), p.as_object()) {
        for (k, v) in from.iter().filter(|(k, _)| k.as_str() != "folder") {
            into.insert(k.clone(), v.clone());
        }
    }
    params["dir"] = json!(folder);
    let r = app.session.execute(command, &params).map_err(|e| e.to_string())?;
    app.status(format!("{} {}", tl!("Films saved to:"), folder));
    Ok(r)
}

/// `ui.screenSeparate`: spot-colour separation, four colours found in the art, registration marks.
pub fn separate(app: &mut VectorcraftApp, p: &Value) -> Result<Value, String> {
    run(app, p, "screen.separate", json!({ "dpi": 600, "colors": 4, "marks": true }))
}

/// `ui.screenHalftone`: one black halftone film with registration marks.
pub fn halftone(app: &mut VectorcraftApp, p: &Value) -> Result<Value, String> {
    run(app, p, "screen.halftone", json!({ "dpi": 600, "marks": true }))
}

/// `ui.screenTestPage`: the print-test films (tone patches, line widths, trap overlaps).
pub fn test_page(app: &mut VectorcraftApp, p: &Value) -> Result<Value, String> {
    run(app, p, "screen.testPage", json!({ "dpi": 600 }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_test_page_menu_item_writes_films_into_the_folder_without_a_document() {
        let mut app = VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        let dir = std::env::temp_dir().join(format!("sf-testpage-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let r = test_page(&mut app, &json!({"folder": dir.to_string_lossy(), "dpi": 100, "parts": ["trap"]})).unwrap();
        assert!(r["films"].as_array().is_some_and(|f| f.len() == 2), "{r}");
        assert!(std::fs::read_dir(&dir).unwrap().count() >= 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

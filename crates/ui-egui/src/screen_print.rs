//! Screen Print menu (SC-1): pick a folder, then run a `screen.*` engine command into it.
//!
//! `ui.screenSeparate` writes one film a screen for the artboard (`screen.separate`);
//! `ui.screenHalftone` writes a halftone film (`screen.halftone`). Any engine parameter can be
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

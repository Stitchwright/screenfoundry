//! Open in ScreenFoundry (SC-11): `?open=<address>` opens a design file from MakerFoundry, so its
//! design pages can link straight into the web app. The address is checked by `sc_maker::link`
//! (https only, MakerFoundry's own hosts, a path under /api/v1/, nothing odd in it) and the page
//! fetches the CHECKED address, never the text that came in: no cookies, no redirects, a time limit
//! and at most 32 MB. Only an SVG opens; the file then opens as a dropped one does (through the
//! inbox). MakerFoundry must allow this page's address (CORS) and expose `Content-Disposition`.

use sc_maker::link::{self, Checked};

use crate::fetch::{self, Options};

/// Fetch the design at `checked` and put it in `inbox`, which opens it; `note` gets the message the
/// status bar shows when it can't be opened.
pub fn start(checked: Checked, inbox: vectorcraft_ui_egui::Inbox, note: std::rc::Rc<std::cell::RefCell<Option<String>>>, ctx: egui::Context) {
    wasm_bindgen_futures::spawn_local(async move {
        match get(&checked).await {
            Ok(file) => inbox.lock().unwrap_or_else(|e| e.into_inner()).push(file),
            Err(e) => {
                log::error!("vectorcraft-web: open {}: {e}", checked.as_str());
                *note.borrow_mut() = Some(format!("Couldn't open the design from MakerFoundry: {e}"));
            }
        }
        ctx.request_repaint();
    });
}

/// The design: its name and bytes, or the sentence saying why not.
async fn get(checked: &Checked) -> Result<(String, Vec<u8>), String> {
    let options = Options { method: "GET", headers: vec![], body: None, seconds: link::TIMEOUT_SECONDS };
    let response = fetch::fetch(checked.as_str(), &options).await?;
    if !response.ok() {
        // MakerFoundry's own sentence (403 for a design for sale, 404, 429...)
        let said = fetch::read_capped(&response, 16 * 1024).await.ok().flatten().unwrap_or_default();
        return Err(link::refusal(response.status(), &String::from_utf8_lossy(&said)));
    }
    let disposition = response.headers().get("content-disposition").ok().flatten();
    let name = link::file_name(checked, disposition.as_deref());
    link::require_svg(&name)?;
    let bytes = fetch::read_capped(&response, link::MAX_BYTES).await?.ok_or("the design is too big to open")?;
    Ok((name, bytes))
}

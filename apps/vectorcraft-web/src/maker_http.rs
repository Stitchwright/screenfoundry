//! The web app's HTTP service for MakerFoundry and Foundry Account (SC-11, `sc-maker`): the browser's
//! `fetch` (`crate::fetch`), for the few requests the app makes and no others. The page's content
//! security policy names the same two sites, and both sites answer the web app's address with CORS.

use sc_maker::{Request, Response};
use vectorcraft_ui_egui::HttpDone;

use crate::fetch::{self, Options};

/// The most of an answer that is read (bytes).
const MAX_ANSWER: usize = 256 * 1024;
/// The whole request may take this long.
const SECONDS: u32 = 60;

/// Send `request` and hand the answer to `done`; repaint when it is in.
pub fn send(request: Request, done: HttpDone, ctx: egui::Context) {
    wasm_bindgen_futures::spawn_local(async move {
        let answer = run(&request).await;
        done(answer);
        ctx.request_repaint();
    });
}

async fn run(request: &Request) -> Result<Response, String> {
    request.expected()?;
    let options = Options { method: request.method, headers: request.headers.clone(), body: Some(request.body.clone()), seconds: SECONDS };
    let response = fetch::fetch(&request.url, &options).await?;
    let status = response.status();
    let body = fetch::read_capped(&response, MAX_ANSWER).await?.unwrap_or_default();
    Ok(Response { status, body: String::from_utf8_lossy(&body).into_owned() })
}

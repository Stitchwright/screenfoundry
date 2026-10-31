//! The browser's `fetch` for MakerFoundry and Foundry Account (SC-11): no cookies, no redirects, a
//! time limit, and a cap on how much of an answer is read. Both the page's own request (the
//! `?open=` design, `crate::open_link`) and the app's (`crate::maker_http`) go through here.

use js_sys::{Object, Reflect, Uint8Array};
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use wasm_bindgen_futures::JsFuture;

/// What a `fetch` needs besides its address.
pub struct Options {
    pub method: &'static str,
    pub headers: Vec<(&'static str, String)>,
    pub body: Option<String>,
    /// Give up after this many seconds.
    pub seconds: u32,
}

/// A text for any error value a script can throw.
pub fn js(e: JsValue) -> String {
    e.as_string().or_else(|| Reflect::get(&e, &"message".into()).ok().and_then(|m| m.as_string())).unwrap_or_else(|| format!("{e:?}"))
}

/// Fetch `url`: credentials omitted, redirects an error, aborted after `options.seconds`.
pub async fn fetch(url: &str, options: &Options) -> Result<web_sys::Response, String> {
    let window = web_sys::window().ok_or("no window")?;
    let init = web_sys::RequestInit::new();
    init.set_method(options.method);
    init.set_mode(web_sys::RequestMode::Cors);
    init.set_credentials(web_sys::RequestCredentials::Omit);
    init.set_redirect(web_sys::RequestRedirect::Error);
    let headers = web_sys::Headers::new().map_err(js)?;
    for (name, value) in &options.headers {
        headers.set(name, value).map_err(js)?;
    }
    init.set_headers(&headers);
    if let Some(body) = &options.body {
        init.set_body(&JsValue::from_str(body));
    }
    let controller = web_sys::AbortController::new().map_err(js)?;
    init.set_signal(Some(&controller.signal()));
    let abort = controller.clone();
    let on_time = Closure::once_into_js(move || abort.abort());
    let timer = window
        .set_timeout_with_callback_and_timeout_and_arguments_0(
            on_time.unchecked_ref(),
            (options.seconds.saturating_mul(1000)).min(i32::MAX as u32) as i32,
        )
        .map_err(js)?;
    let answer = JsFuture::from(window.fetch_with_str_and_init(url, &init)).await;
    window.clear_timeout_with_handle(timer);
    answer.map_err(|_| "the request failed (no answer, too slow, or not allowed by the site)".to_string())?.dyn_into().map_err(js)
}

/// The body of `response`, at most `max` bytes: reading stops, and the rest is cancelled, as soon as
/// more than that has come, so a huge answer is never held whole. `None` when it was longer.
pub async fn read_capped(response: &web_sys::Response, max: usize) -> Result<Option<Vec<u8>>, String> {
    let too_long = response.headers().get("content-length").ok().flatten().and_then(|n| n.parse::<u64>().ok()).is_some_and(|n| n > max as u64);
    if too_long {
        return Ok(None);
    }
    let Some(stream) = response.body() else { return Ok(Some(Vec::new())) };
    let reader: web_sys::ReadableStreamDefaultReader = stream.get_reader().unchecked_into();
    let mut bytes: Vec<u8> = Vec::new();
    loop {
        let step: Object = JsFuture::from(reader.read()).await.map_err(js)?.unchecked_into();
        if Reflect::get(&step, &"done".into()).map_err(js)?.as_bool().unwrap_or(true) {
            return Ok(Some(bytes));
        }
        let chunk: Uint8Array = Reflect::get(&step, &"value".into()).map_err(js)?.dyn_into().map_err(|_| "the answer wasn't bytes".to_string())?;
        if bytes.len().saturating_add(chunk.length() as usize) > max {
            let _ = reader.cancel();
            return Ok(None);
        }
        bytes.extend_from_slice(&chunk.to_vec());
    }
}

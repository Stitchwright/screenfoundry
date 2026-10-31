//! The desktop app's HTTP client for MakerFoundry and Foundry Account (SC-11, `sc-maker`): one
//! request on a thread of its own, over HTTPS (certificates checked against the system's), with no
//! cookies, no redirects, a time limit, and a cap on the answer's size. Whatever the service
//! answers is handed back as text; `sc-maker` reads it.

use std::io::Read;
use std::time::Duration;

use sc_maker::{Request, Response};
use vectorcraft_ui_egui::HttpDone;

/// The whole request may take this long.
const TIMEOUT: Duration = Duration::from_secs(60);
/// The most of an answer that is read (bytes): MakerFoundry's and the account service's answers are
/// small JSON documents.
const MAX_ANSWER: u64 = 256 * 1024;

/// Send `request` on a thread and hand the answer to `done`.
pub(crate) fn send(request: Request, done: HttpDone) {
    // only the few requests the app makes
    if let Err(e) = request.expected() {
        done(Err(e));
        return;
    }
    let spawned = std::thread::Builder::new().name("maker-http".into()).spawn(move || done(request_blocking(&request)));
    if let Err(e) = spawned {
        log::error!("could not start the request thread: {e}");
    }
}

/// Send `request` and wait for the answer.
fn request_blocking(request: &Request) -> Result<Response, String> {
    request_with(request, true)
}

/// [`request_blocking`], choosing whether the system's proxy settings apply (the tests talk to a
/// server on this machine).
fn request_with(request: &Request, system_proxy: bool) -> Result<Response, String> {
    let config = ureq::Agent::config_builder()
        .proxy(if system_proxy { ureq::Proxy::try_from_env() } else { None })
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_global(Some(TIMEOUT))
        .user_agent(concat!("ScreenFoundry/", env!("CARGO_PKG_VERSION")))
        .build();
    let agent: ureq::Agent = config.into();
    let mut builder = ureq::http::Request::builder().method(request.method).uri(request.url.as_str());
    for (name, value) in &request.headers {
        builder = builder.header(*name, value.as_str());
    }
    let http = builder.body(request.body.clone()).map_err(|e| e.to_string())?;
    let mut answer = agent.run(http).map_err(|e| describe(&e))?;
    let status = answer.status().as_u16();
    let mut body = String::new();
    answer.body_mut().as_reader().take(MAX_ANSWER).read_to_string(&mut body).unwrap_or_default();
    Ok(Response { status, body })
}

/// A failure to get an answer, as a sentence for the person.
fn describe(e: &ureq::Error) -> String {
    match e {
        ureq::Error::Timeout(_) => "The service took too long to answer. Try again later.".into(),
        ureq::Error::HostNotFound | ureq::Error::ConnectionFailed | ureq::Error::Io(_) => "Couldn't reach the service: check your internet connection.".into(),
        other => format!("The request failed: {other}"),
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;

    use super::*;

    /// A one-answer HTTP server on this machine: it sends `reply` to the first request and returns
    /// what that request was.
    fn serve(reply: &'static str) -> (String, mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            // the head, then as many body bytes as its Content-Length says
            let mut got: Vec<u8> = vec![];
            let mut chunk = [0u8; 4096];
            loop {
                let n = socket.read(&mut chunk).unwrap_or(0);
                got.extend_from_slice(&chunk[..n]);
                let text = String::from_utf8_lossy(&got).to_ascii_lowercase();
                let done = text.split_once("\r\n\r\n").is_some_and(|(head, body)| {
                    let want = head.split("content-length:").nth(1).and_then(|v| v.split("\r\n").next()).and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(0);
                    body.len() >= want
                });
                if n == 0 || done {
                    break;
                }
            }
            // (a test that doesn't look at the request has dropped the receiver)
            let _ = tx.send(String::from_utf8_lossy(&got).into_owned());
            let _ = socket.write_all(reply.as_bytes());
        });
        (url, rx)
    }

    fn post(url: &str) -> Request {
        Request {
            method: "POST",
            url: format!("{url}/api/v1/designs"),
            headers: vec![("Authorization", "Bearer sfs_t".into()), ("Content-Type", "application/json".into())],
            body: r#"{"a":1}"#.into(),
        }
    }

    #[test]
    fn the_request_goes_out_as_built_and_the_answer_comes_back() {
        let (url, seen) = serve("HTTP/1.1 201 Created\r\nContent-Type: application/json\r\nContent-Length: 12\r\nConnection: close\r\n\r\n{\"id\": 42}\r\n");
        let r = request_with(&post(&url), false).unwrap();
        assert_eq!((r.status, r.body.trim()), (201, "{\"id\": 42}"));
        let request = seen.recv().unwrap().to_ascii_lowercase();
        assert!(request.starts_with("post /api/v1/designs http/1.1"), "{request}");
        assert!(request.contains("authorization: bearer sfs_t") && request.contains("content-type: application/json"), "{request}");
        assert!(request.contains("user-agent: screenfoundry/") && !request.contains("cookie"), "{request}");
        assert!(request.ends_with("{\"a\":1}"), "{request}");
    }

    #[test]
    fn a_refusal_is_an_answer_not_an_error() {
        let (url, _) = serve("HTTP/1.1 429 Too Many Requests\r\nContent-Length: 40\r\nConnection: close\r\n\r\n{\"error\":{\"message\":\"Too many designs.\"}}");
        let r = request_with(&post(&url), false).unwrap();
        assert_eq!(r.status, 429);
        assert!(r.body.contains("Too many designs."));
    }

    #[test]
    fn a_redirect_is_not_followed() {
        let (url, _) = serve("HTTP/1.1 302 Found\r\nLocation: https://evil.example/\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        let r = request_with(&post(&url), false).unwrap();
        assert_eq!(r.status, 302, "the redirect itself is the answer");
    }

    #[test]
    fn an_answer_is_cut_off_at_the_cap() {
        let big = Box::leak(format!("HTTP/1.1 200 OK\r\nContent-Length: 600000\r\nConnection: close\r\n\r\n{}", "x".repeat(600_000)).into_boxed_str());
        let (url, _) = serve(big);
        let r = request_with(&post(&url), false).unwrap();
        assert!(r.body.len() as u64 <= MAX_ANSWER, "{}", r.body.len());
    }

    #[test]
    fn a_request_the_app_does_not_make_is_never_sent() {
        let (url, seen) = serve("HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        let (tx, rx) = mpsc::channel();
        send(post(&url), Box::new(move |r| tx.send(r).unwrap()));
        assert!(rx.recv().unwrap().is_err(), "a local address is not one of the two services");
        assert!(seen.recv_timeout(std::time::Duration::from_millis(300)).is_err(), "nothing reached the server");
    }

    #[test]
    fn nothing_listening_is_a_sentence() {
        let closed = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap();
        let e = request_with(&post(&format!("http://{closed}")), false).unwrap_err();
        assert!(e.contains("Couldn't reach") || e.contains("failed"), "{e}");
    }
}

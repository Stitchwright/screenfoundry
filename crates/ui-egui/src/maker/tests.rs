//! Publish to MakerFoundry and Foundry Account, with a host that records the requests and answers
//! from a script (no network).

use std::cell::RefCell;
use std::rc::Rc;

use sc_maker::{Request, Response};
use serde_json::{Value, json};
use vectorcraft_engine::Session;

use super::*;
use crate::Services;

type Seen = Rc<RefCell<Vec<Request>>>;
type Opened = Rc<RefCell<Vec<String>>>;

/// An app whose host answers each request with the next scripted answer, and records what was sent
/// and which addresses were opened in the browser.
fn app(answers: Vec<Result<Response, String>>) -> (VectorcraftApp, Seen, Opened) {
    let (seen, opened): (Seen, Opened) = Default::default();
    let (s, o) = (seen.clone(), opened.clone());
    let mut answers = answers.into_iter();
    let services = Services {
        http: Some(Box::new(move |request, done| {
            s.borrow_mut().push(request);
            done(answers.next().unwrap_or_else(|| Err("no more scripted answers".into())));
        })),
        open_url: Some(Box::new(move |url: &str| o.borrow_mut().push(url.to_string()))),
        ..Default::default()
    };
    let mut app = VectorcraftApp::new(Session::new(), services);
    app.run("file.new", json!({"width": 100, "height": 80, "name": "Shirt.svg"})).unwrap();
    app.run("shape.rectangle", json!({"x": 10, "y": 10, "width": 30, "height": 20})).unwrap();
    (app, seen, opened)
}

fn reply(status: u16, body: Value) -> Result<Response, String> {
    Ok(Response { status, body: body.to_string() })
}

fn signed_in() -> Result<Response, String> {
    reply(200, json!({ "token": "sfs_tok-1", "account": { "email": "me@example.com", "credits": 100 } }))
}

fn draft() -> Result<Response, String> {
    reply(201, json!({ "id": 42, "url": "https://makerfoundry.app/designs/42", "kind": "laser", "format": "SVG" }))
}

fn kind(app: &VectorcraftApp) -> Option<&str> {
    app.ui.dialog.as_ref().map(|d| d.kind.as_str())
}

fn type_credentials(app: &mut VectorcraftApp) {
    let d = app.ui.dialog.as_mut().unwrap();
    d.fields.insert("email".into(), json!("me@example.com"));
    d.fields.insert("password".into(), json!("correct horse"));
}

/// Press OK and read the answer.
fn press_ok(app: &mut VectorcraftApp) -> Result<Value, String> {
    let r = crate::dialogs::confirm(app);
    drain(app);
    r
}

#[test]
fn publishing_signed_out_asks_to_sign_in_then_shows_what_is_sent_and_only_then_sends() {
    let (mut app, seen, opened) = app(vec![signed_in(), draft()]);
    app.run("ui.publishToMakerFoundry", json!({})).unwrap();
    assert_eq!(kind(&app), Some(ACCOUNT_KIND));
    assert!(seen.borrow().is_empty(), "nothing is sent before the person acts");
    type_credentials(&mut app);
    press_ok(&mut app).unwrap();
    {
        let sent = seen.borrow();
        assert_eq!((sent[0].method, sent[0].url.as_str()), ("POST", "https://cloud.stitchfoundry.app/v1/signin"));
        assert!(sent[0].body.contains("correct horse"));
    }
    // signed in: the Publish dialog shows the file and where it goes; still nothing sent to MakerFoundry
    assert_eq!(kind(&app), Some(PUBLISH_KIND));
    assert_eq!(app.maker.email(), Some("me@example.com"));
    assert_eq!(app.ui.dialog.as_ref().unwrap().str("name"), "Shirt", "the document's name, to change");
    // MakerFoundry makes the draft's title from the file name: the person can name it
    app.ui.dialog.as_mut().unwrap().fields.insert("name".into(), json!("Tiger shirt front"));
    assert_eq!(seen.borrow().len(), 1);
    assert!(opened.borrow().is_empty());
    // Send
    press_ok(&mut app).unwrap();
    let sent = seen.borrow();
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[1].url, "https://makerfoundry.app/api/v1/designs");
    assert!(sent[1].headers.contains(&("Authorization", "Bearer sfs_tok-1".into())));
    let body: Value = serde_json::from_str(&sent[1].body).unwrap();
    assert_eq!(body["file_name"], "Tiger shirt front.svg");
    assert!(body["app"].as_str().unwrap().starts_with("ScreenFoundry "));
    let svg = vectorcraft_format::base64_decode(body["file"].as_str().unwrap()).unwrap();
    assert!(String::from_utf8(svg).unwrap().contains("<svg"));
    // the draft's page opens, and the dialog is gone
    assert_eq!(*opened.borrow(), ["https://makerfoundry.app/designs/42"]);
    assert!(app.ui.dialog.is_none() && app.maker.waiting().is_none());
}

#[test]
fn a_blank_or_hostile_name_still_gives_a_plain_svg_name() {
    for (typed, want) in [("", "Shirt.svg"), ("   ", "Shirt.svg"), ("../../etc/passwd", "passwd.svg"), ("a\r\nb", "ab.svg"), (".svg", "design.svg")] {
        let (mut app, seen, _) = app(vec![signed_in(), draft()]);
        app.run("ui.publishToMakerFoundry", json!({})).unwrap();
        type_credentials(&mut app);
        press_ok(&mut app).unwrap();
        app.ui.dialog.as_mut().unwrap().fields.insert("name".into(), json!(typed));
        press_ok(&mut app).unwrap();
        let body: Value = serde_json::from_str(&seen.borrow()[1].body).unwrap();
        assert_eq!(body["file_name"], want, "{typed:?}");
    }
}

#[test]
fn the_command_alone_never_sends_and_the_password_and_token_are_not_kept() {
    let (mut app, seen, _) = app(vec![signed_in()]);
    app.run("ui.publishToMakerFoundry", json!({})).unwrap();
    app.run("ui.publishToMakerFoundry", json!({"confirm": true, "send": true})).unwrap();
    assert!(seen.borrow().is_empty());
    type_credentials(&mut app);
    press_ok(&mut app).unwrap();
    // the password was sent once and is gone from the dialog; the token is in no saved setting
    assert!(app.ui.dialog.as_ref().is_none_or(|d| d.str("password").is_empty()));
    let saved = serde_json::to_string(&app.ui).unwrap();
    assert!(!saved.contains("sfs_tok-1") && !saved.contains("correct horse"));
}

#[test]
fn an_empty_drawing_is_refused_before_anything_opens() {
    let (mut app, seen, _) = app(vec![]);
    app.run("select.all", json!({})).unwrap();
    app.run("edit.clear", json!({})).unwrap();
    let e = app.run("ui.publishToMakerFoundry", json!({})).unwrap_err();
    assert!(e.contains("empty"), "{e}");
    assert!(app.ui.dialog.is_none() && seen.borrow().is_empty());
}

#[test]
fn makerfoundrys_own_sentence_is_shown_and_nothing_opens() {
    let (mut app, _, opened) = app(vec![signed_in(), reply(429, json!({"error": {"message": "You have published too many designs today."}}))]);
    app.run("ui.publishToMakerFoundry", json!({})).unwrap();
    type_credentials(&mut app);
    press_ok(&mut app).unwrap();
    press_ok(&mut app).unwrap();
    assert_eq!(kind(&app), Some(PUBLISH_KIND));
    assert_eq!(app.ui.dialog.as_ref().unwrap().str("error"), "You have published too many designs today.");
    assert!(opened.borrow().is_empty());
}

#[test]
fn a_draft_page_on_another_site_is_never_opened() {
    for url in [
        "https://evil.example/designs/42",
        "https://makerfoundry.app.evil.example/designs/42",
        "javascript:alert(1)",
        "https://makerfoundry.app@evil.example/",
    ] {
        let (mut app, _, opened) = app(vec![signed_in(), reply(201, json!({"id": 42, "url": url}))]);
        app.run("ui.publishToMakerFoundry", json!({})).unwrap();
        type_credentials(&mut app);
        press_ok(&mut app).unwrap();
        press_ok(&mut app).unwrap();
        assert!(opened.borrow().is_empty(), "{url}");
        assert!(app.ui.dialog.as_ref().unwrap().str("error").contains("another site"), "{url}");
    }
}

#[test]
fn an_ended_sign_in_goes_back_to_the_sign_in_dialog_and_keeps_the_design() {
    let (mut app, _, _) = app(vec![signed_in(), reply(401, json!({})), signed_in(), draft()]);
    app.run("ui.publishToMakerFoundry", json!({})).unwrap();
    type_credentials(&mut app);
    press_ok(&mut app).unwrap();
    press_ok(&mut app).unwrap();
    assert_eq!(kind(&app), Some(ACCOUNT_KIND));
    assert!(!app.maker.signed_in());
    assert!(app.ui.dialog.as_ref().unwrap().str("error").contains("Sign in again"));
    type_credentials(&mut app);
    press_ok(&mut app).unwrap();
    assert_eq!(kind(&app), Some(PUBLISH_KIND), "the same design is offered again");
}

#[test]
fn a_wrong_password_shows_the_services_sentence_and_stays_signed_out() {
    let (mut app, _, _) = app(vec![reply(401, json!({"error": {"message": "Wrong email or password."}}))]);
    app.run("ui.foundryAccount", json!({})).unwrap();
    type_credentials(&mut app);
    press_ok(&mut app).unwrap();
    assert!(!app.maker.signed_in());
    assert_eq!(app.ui.dialog.as_ref().unwrap().str("error"), "Wrong email or password.");
}

#[test]
fn create_account_and_sign_out_use_the_account_service() {
    let (mut app, seen, _) = app(vec![signed_in(), reply(200, json!({}))]);
    app.run("ui.foundryAccount", json!({})).unwrap();
    type_credentials(&mut app);
    app.ui.dialog.as_mut().unwrap().fields.insert("create".into(), json!(true));
    press_ok(&mut app).unwrap();
    assert_eq!(seen.borrow()[0].url, "https://cloud.stitchfoundry.app/v1/signup");
    assert!(app.maker.signed_in() && app.ui.dialog.is_none());
    app.run("ui.foundryAccount", json!({})).unwrap();
    press_ok(&mut app).unwrap();
    assert_eq!(seen.borrow()[1].url, "https://cloud.stitchfoundry.app/v1/signout");
    assert!(seen.borrow()[1].headers.contains(&("Authorization", "Bearer sfs_tok-1".into())));
    assert!(!app.maker.signed_in());
}

#[test]
fn a_host_that_cannot_send_is_told_so_and_the_menu_items_are_disabled() {
    let mut app = VectorcraftApp::new(Session::new(), Services::default());
    app.run("file.new", json!({"width": 100, "height": 80})).unwrap();
    app.run("shape.rectangle", json!({"x": 10, "y": 10, "width": 30, "height": 20})).unwrap();
    let entries = crate::menus::menu_entries(&app);
    for id in ["ui.publishToMakerFoundry", "ui.foundryAccount"] {
        let e = entries.iter().find(|e| e.command.as_deref() == Some(id)).unwrap();
        assert!(!e.enabled, "{id}");
    }
    app.maker.session = None;
    app.ui.dialog = Some(Dialog::new(ACCOUNT_KIND, json!({"email": "me@example.com", "password": "pw"})));
    let e = crate::dialogs::confirm(&mut app).unwrap_err();
    assert!(e.contains("can't be reached"), "{e}");
}

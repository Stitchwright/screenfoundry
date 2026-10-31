//! MakerFoundry (SC-11): File › Publish to MakerFoundry… and File › Foundry Account….
//!
//! Publishing sends the drawing, exported as an SVG, to MakerFoundry as a private draft, signed in
//! with the person's Foundry Account. The dialog shows what goes where first, and the request is
//! sent only when the person presses Send in it: no command sends without that (so a script or an
//! assistant can open the dialog but not press its button). MakerFoundry's page for the draft opens
//! in the browser only when it is on the site the request went to.
//!
//! The Foundry Account sign-in is held in memory for this run (the session token, never the
//! password, and never written to the settings or any file); signing in again is one dialog. The
//! requests and the checks on every answer are `sc-maker`'s; the host sends them
//! ([`Services::http`](crate::Services::http)).

use std::sync::{Arc, Mutex};

use sc_maker::account::{Accounts, Session};
use sc_maker::publish::Publish;
use sc_maker::{Request, Response, Site};
use serde_json::{Value, json};

use crate::VectorcraftApp;
use crate::state::Dialog;

/// The Foundry Account dialog's kind.
pub(crate) const ACCOUNT_KIND: &str = "foundryAccount";
/// The Publish to MakerFoundry dialog's kind.
pub(crate) const PUBLISH_KIND: &str = "makerFoundryPublish";

/// The answer to the request on its way, put there by the host's thread.
type Mailbox = Arc<Mutex<Option<Result<Response, String>>>>;

/// What the request on its way is for.
enum Pending {
    SignIn,
    SignOut,
    Publish(Publish),
}

/// The design the Publish dialog is about to send.
#[derive(Clone, Debug)]
struct Prepared {
    file_name: String,
    file_base64: String,
    bytes: u64,
    app: String,
}

/// The account and the request in flight, between frames.
#[derive(Default)]
pub struct MakerState {
    session: Option<Session>,
    pending: Option<Pending>,
    mailbox: Mailbox,
    prepared: Option<Prepared>,
    /// Open the Publish dialog once the sign-in the person was sent to has worked.
    publish_next: bool,
}

impl MakerState {
    /// The signed-in account's email, when signed in.
    pub fn email(&self) -> Option<&str> {
        self.session.as_ref().map(|s| s.email.as_str())
    }

    /// The credits left, as the service last said.
    pub fn credits(&self) -> Option<u64> {
        self.session.as_ref().and_then(|s| s.credits)
    }

    /// Whether the person is signed in to Foundry Account.
    pub fn signed_in(&self) -> bool {
        self.session.is_some()
    }

    /// Whether a request is on its way.
    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }

    /// The design waiting in the Publish dialog: its file name and size in bytes.
    pub fn waiting(&self) -> Option<(&str, u64)> {
        self.prepared.as_ref().map(|p| (p.file_name.as_str(), p.bytes))
    }
}

/// `ui.publishToMakerFoundry`: export the drawing and ask the person to confirm sending it. Signed
/// out, the Foundry Account dialog comes first and the Publish dialog follows the sign-in.
pub fn publish(app: &mut VectorcraftApp, p: &Value) -> Result<Value, String> {
    let mut params = json!({});
    if let Some(name) = p.get("fileName") {
        params["fileName"] = name.clone();
    }
    let exported = app.session.execute("screen.makerFoundryExport", &params).map_err(|e| e.to_string())?;
    let text = |key: &str| exported.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
    let prepared = Prepared {
        file_name: text("fileName"),
        file_base64: text("fileBase64"),
        bytes: exported.get("bytes").and_then(Value::as_u64).unwrap_or_default(),
        app: text("app"),
    };
    app.maker.prepared = Some(prepared);
    if app.maker.signed_in() {
        open_publish(app);
    } else {
        app.maker.publish_next = true;
        open_account(app);
    }
    Ok(Value::Null)
}

/// `ui.foundryAccount`: the Foundry Account dialog (sign in, create an account, sign out).
pub fn account(app: &mut VectorcraftApp, _p: &Value) -> Result<Value, String> {
    app.maker.publish_next = false;
    open_account(app);
    Ok(Value::Null)
}

fn open_account(app: &mut VectorcraftApp) {
    app.ui.dialog = Some(Dialog::new(ACCOUNT_KIND, json!({ "email": "", "password": "", "error": "" })));
}

fn open_publish(app: &mut VectorcraftApp) {
    app.ui.dialog = Some(Dialog::new(PUBLISH_KIND, json!({ "error": "" })));
}

/// The Foundry Account dialog's OK: sign in or create an account, or sign out when signed in.
pub(crate) fn confirm_account(app: &mut VectorcraftApp, d: &Dialog) -> Result<Value, String> {
    let accounts = Accounts::new(Site::accounts());
    if let Some(session) = app.maker.session.take() {
        // signed out here at once; the service is told as well, and a failure to tell it is not shown
        if let Ok(request) = accounts.sign_out(&session.token) {
            let _ = send(app, request, Pending::SignOut);
        }
        app.ui.dialog = None;
        return Ok(Value::Null);
    }
    let (email, password) = (d.str("email"), d.str("password"));
    let request = if d.bool("create") { accounts.sign_up(&email, &password) } else { accounts.sign_in(&email, &password) }?;
    // the password is sent once and forgotten
    set_field(app, ACCOUNT_KIND, "password", json!(""));
    set_field(app, ACCOUNT_KIND, "create", json!(false));
    send(app, request, Pending::SignIn)?;
    set_field(app, ACCOUNT_KIND, "error", json!(""));
    Ok(Value::Null)
}

/// The Publish dialog's Send: sign the request with the session and send it.
pub(crate) fn confirm_publish(app: &mut VectorcraftApp, _d: &Dialog) -> Result<Value, String> {
    let token = app.maker.session.as_ref().map(|s| s.token.clone()).ok_or_else(|| tl!("Sign in to your Foundry Account to publish.").to_string())?;
    let prepared = app.maker.prepared.clone().ok_or("there is no design to publish")?;
    let publish = Publish::new(Site::makerfoundry(), &prepared.file_name, prepared.file_base64, &prepared.app)?;
    let request = publish.request(&token)?;
    send(app, request, Pending::Publish(publish))?;
    set_field(app, PUBLISH_KIND, "error", json!(""));
    app.status(tl!("Sending to MakerFoundry…"));
    Ok(Value::Null)
}

/// Hand `request` to the host; its answer is read by [`drain`].
fn send(app: &mut VectorcraftApp, request: Request, what: Pending) -> Result<(), String> {
    if app.maker.pending.is_some() {
        return Err("the last request hasn't finished yet".into());
    }
    let Some(http) = app.services.http.as_mut() else {
        return Err("MakerFoundry can't be reached from this version of ScreenFoundry".into());
    };
    let mailbox = app.maker.mailbox.clone();
    http(
        request,
        Box::new(move |answer| {
            *mailbox.lock().unwrap_or_else(|e| e.into_inner()) = Some(answer);
        }),
    );
    app.maker.pending = Some(what);
    Ok(())
}

fn set_field(app: &mut VectorcraftApp, kind: &str, key: &str, value: Value) {
    if let Some(d) = app.ui.dialog.as_mut().filter(|d| d.kind == kind) {
        d.fields.insert(key.into(), value);
    }
}

/// Read the answer that came, if any (called every frame).
pub fn drain(app: &mut VectorcraftApp) {
    if app.maker.pending.is_none() {
        return;
    }
    let Some(answer) = app.maker.mailbox.lock().unwrap_or_else(|e| e.into_inner()).take() else { return };
    let Some(what) = app.maker.pending.take() else { return };
    match what {
        Pending::SignIn => signed_in(app, answer),
        Pending::SignOut => {}
        Pending::Publish(publish) => published(app, &publish, answer),
    }
}

fn signed_in(app: &mut VectorcraftApp, answer: Result<Response, String>) {
    let result = answer.and_then(|r| Accounts::new(Site::accounts()).session(&r));
    match result {
        Ok(session) => {
            app.status(crate::i18n::fmt(tl!("Signed in as {email}"), &[("email", &session.email)]));
            app.maker.session = Some(session);
            if std::mem::take(&mut app.maker.publish_next) {
                open_publish(app);
            } else {
                app.ui.dialog = None;
            }
        }
        Err(e) => {
            set_field(app, ACCOUNT_KIND, "error", json!(e));
            app.status(e);
        }
    }
}

fn published(app: &mut VectorcraftApp, publish: &Publish, answer: Result<Response, String>) {
    let unauthorized = answer.as_ref().is_ok_and(|r| r.status == 401);
    match answer.and_then(|r| publish.answer(&r)) {
        Ok(draft) => {
            app.maker.prepared = None;
            app.ui.dialog = None;
            app.open_url(&draft.url);
            app.status(tl!("Your draft is open on MakerFoundry: add its title, price and licence there, and publish it."));
        }
        Err(e) => {
            let shown = e.clone();
            if unauthorized {
                // the sign-in has ended: ask for it again, then offer the same design
                app.maker.session = None;
                app.maker.publish_next = true;
                open_account(app);
                set_field(app, ACCOUNT_KIND, "error", json!(e));
            } else {
                set_field(app, PUBLISH_KIND, "error", json!(e));
            }
            app.status(shown);
        }
    }
}

#[cfg(test)]
mod tests;

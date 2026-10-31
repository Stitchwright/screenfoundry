//! Publish to MakerFoundry and Foundry Account (SC-11, `crate::maker`).
//!
//! Publish shows what is sent and where (the file's name and size, MakerFoundry's address, the
//! account it goes with), and Send is the only thing that sends it. Foundry Account takes an email
//! and a password to sign in or create an account (a new account starts with free credits and
//! sends no email), and signs out when signed in.

use super::{DialogSpec, form};
use crate::state::Dialog;
use crate::theme::Tokens;
use crate::{VectorcraftApp, widgets};

/// Width of the text fields.
const FIELD: f32 = 280.0;
/// Width of the labels.
const LABEL: f32 = 80.0;

pub(super) const ACCOUNT: DialogSpec = DialogSpec {
    heading: |_| tl!("Foundry Account").into(),
    body: account_body,
    confirm: crate::maker::confirm_account,
    ok: Some("Sign In"),
    ok_label: Some(|app| if app.maker.signed_in() { "Sign Out" } else { "Sign In" }),
    ok_enabled: Some(|app, d| !app.maker.busy() && (app.maker.signed_in() || (!d.str("email").trim().is_empty() && !d.str("password").is_empty()))),
    alternate: Some(("Create Account", "create")),
    min_width: 420.0,
    max_width: Some(460.0),
    ..DialogSpec::FORM
};

pub(super) const PUBLISH: DialogSpec = DialogSpec {
    heading: |_| tl!("Publish to MakerFoundry").into(),
    body: publish_body,
    confirm: crate::maker::confirm_publish,
    ok: Some("Send"),
    ok_enabled: Some(|app, _| !app.maker.busy() && app.maker.signed_in()),
    min_width: 460.0,
    max_width: Some(500.0),
    ..DialogSpec::FORM
};

fn account_body(app: &mut VectorcraftApp, ui: &mut egui::Ui, d: &mut Dialog) -> bool {
    let t = Tokens::get(ui.ctx());
    if let Some(email) = app.maker.email() {
        ui.label(crate::i18n::fmt(tl!("Signed in as {email}"), &[("email", email)]));
        if let Some(n) = app.maker.credits() {
            ui.add_space(4.0);
            ui.label(crate::i18n::fmt(tl!("{n} credits"), &[("n", &n.to_string())]));
        }
    } else {
        ui.label(tl!("Sign in to your Foundry Account to publish."));
        ui.add_space(8.0);
        widgets::label_row(ui, tl!("Email"), LABEL, |ui| {
            form::text(ui, d, "email", FIELD);
        });
        ui.add_space(4.0);
        widgets::label_row(ui, tl!("Password"), LABEL, |ui| {
            form::password(ui, d, "password", FIELD);
        });
    }
    status_lines(app, ui, d, &t);
    false
}

fn publish_body(app: &mut VectorcraftApp, ui: &mut egui::Ui, d: &mut Dialog) -> bool {
    let t = Tokens::get(ui.ctx());
    ui.label(tl!(
        "Send this design to MakerFoundry as a draft. Its page opens in your browser: add the title, price and licence there, and publish it."
    ));
    ui.add_space(8.0);
    if let Some((name, bytes)) = app.maker.waiting() {
        // What goes where: the file, the site, the account.
        ui.label(egui::RichText::new(format!("{name}  ({} KB)", bytes.div_ceil(1024))).strong());
        ui.label(egui::RichText::new(sc_maker::Site::MAKERFOUNDRY.trim_start_matches("https://")).color(t.text_dim));
    }
    if let Some(email) = app.maker.email() {
        ui.label(crate::i18n::fmt(tl!("Signed in as {email}"), &[("email", email)]));
    }
    status_lines(app, ui, d, &t);
    false
}

/// The wait and the answer's sentence under the fields.
fn status_lines(app: &VectorcraftApp, ui: &mut egui::Ui, d: &Dialog, t: &Tokens) {
    if app.maker.busy() {
        ui.add_space(8.0);
        ui.label(egui::RichText::new(tl!("Sending to MakerFoundry…")).color(t.text_dim));
    }
    let error = d.str("error");
    if !error.is_empty() {
        ui.add_space(8.0);
        ui.label(egui::RichText::new(error).color(t.error));
    }
}

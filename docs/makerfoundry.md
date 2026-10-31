# MakerFoundry

Status: 2026-10-31. Task **SC-11**: built, tested, and checked live and in Chromium (see below); the hosted page and MakerFoundry's button are the open items.

[MakerFoundry](https://makerfoundry.app) is the Foundry family's marketplace for designs. ScreenFoundry works with it in two ways. MakerFoundry's side of the contract is its
[`docs/api.md`](https://github.com/SiteSentrix/makerfoundry/blob/main/docs/api.md). The code here is `crates/sc-maker` (requests, answers and the address checks; no network code),
`crates/engine/src/cmd/makerfoundry.rs` (the export), `crates/ui-egui/src/maker.rs` and `dialogs/maker.rs` (the dialogs), `apps/vectorcraft/src/http.rs` (the desktop's HTTPS client) and
`apps/vectorcraft-web` (the browser's `fetch`, `open_link.rs`).

**What it sends.** ScreenFoundry has no screen-printing kind on MakerFoundry to publish films to (owner's decision 2026-10-31: SVG now, ask MakerFoundry for the kind later). So it publishes the
drawing as an **SVG**, which MakerFoundry lists as a laser/vector design. Open from a link is **SVG only** as well.

## Publish to MakerFoundry (File menu)
`File › Publish to MakerFoundry…` (`ui.publishToMakerFoundry`):
1. `screen.makerFoundryExport` exports the drawing as an SVG and checks it: an empty drawing or one over 8 MB is refused; a plain `.svg` file name is made from the document's title.
2. Signed out, the **Foundry Account** dialog comes first (`File › Foundry Account…`, `ui.foundryAccount`): email and password to sign in or create an account (sign-up sends no email and gives 100 credits).
3. The **Publish** dialog shows the file's name and size, MakerFoundry's address, and the account it goes with. **Send is the only thing that sends it**: no command and no parameter sends without the
   person pressing it, so a script or an assistant can open the dialog but not press its button.
4. `POST https://makerfoundry.app/api/v1/designs` with `Authorization: Bearer <session token>` and `{file_name, file (base64), app}`. MakerFoundry keeps a private **draft**; its page opens in the
   browser **only if it is on the same site the request went to**, and the person adds the title, licence and price there. Nothing is public until they publish it on MakerFoundry.
5. MakerFoundry's own sentence is shown as it is when it refuses (too many designs today, too big, not readable). A `401` goes back to the sign-in with the same design kept.

The Foundry Account service is `https://cloud.stitchfoundry.app` (`/v1/signup`, `/v1/signin`, `/v1/account`, `/v1/signout`). The session token (`sfs_...`) is **kept in memory for this run only**: never in the
settings or a file, and the password is sent once and cleared from the dialog. (Keeping the sign-in between runs needs the system's secret store; a later task.) Only an `sfs_` token is ever put in a request.

**What a host may send.** Both hosts (the desktop's `ureq` client, the browser's `fetch`) first ask `sc_maker::Request::expected()`: only a `POST` of JSON to MakerFoundry's design address or one of the
account service's four, with exactly the headers each takes. Anything else is refused before it leaves, so a mistake elsewhere can't send a token anywhere else. Requests carry no cookies, follow no
redirects, have a time limit and a cap on how much of the answer is read.

## Open in ScreenFoundry (a link on a MakerFoundry page, web app only)
MakerFoundry's **Open in ScreenFoundry** button links to `https://stitchwright.github.io/screenfoundry/?open=<the design file's address, URL-encoded>`. The web app reads it once at start and:
1. checks the address strictly (`sc_maker::link::open_link`): `https`, host exactly `makerfoundry.app` or `www.makerfoundry.app` (no user name, port or trailing dot), a path under `/api/v1/` whose
   parts decode to plain names (no `.`, `..`, separator, control or hidden character, no second layer of `%XX`), a plain query, no fragment, no spaces, control characters, backslashes or non-ASCII,
   at most 2,048 characters;
2. takes `?open=` out of the page's address (`history.replaceState`), so a reload doesn't fetch it again, and fetches **the checked address**, not the text that came in, with a plain `GET`: no
   cookies (`credentials: "omit"`), no redirects (`redirect: "error"`), 30 seconds, at most 32 MB (the stream is cancelled as soon as more has come);
3. names the file from `Content-Disposition` (`filename*` first) or the end of the address, always a plain name (last part only, no hidden characters, no leading dots, 120 characters); accepts **only
   `.svg`**; and opens it as if it had been dropped on the page. A refusal (`403` for sale, `404`, `429`) is shown in MakerFoundry's own words.

The desktop app has no link handler: the button opens the browser version.

## The web page's content security policy
`apps/vectorcraft-web/index.html` carries a policy that names `https://makerfoundry.app`, `https://www.makerfoundry.app` and `https://cloud.stitchfoundry.app` in `connect-src`. Trunk writes an inline
module script whose text changes with every build, so the policy has `__SCRIPT_HASHES__` and the Trunk `post_build` hook `cargo xtask web-csp` fills it with each inline script's SHA-256 and checks the page.
`cargo xtask web-csp --check <built index.html>` (in the Pages workflow) and the xtask tests fail when a script isn't allowed, when `script-src` allows `unsafe-inline`/`unsafe-eval`, or when `connect-src`
lacks the three sites: a stale hash leaves the page blank (LaserFoundry's hosted app was blank for 12 days that way).

## What was checked
- `sc-maker` unit tests with look-alike hosts (`makerfoundry.app.evil.com`, `makerfoundry.app@evil.com`, ports, trailing dot, unicode and punycode), schemes, traversal (`..`, `%2e%2e`, `%252e`, encoded
  separators), encoded and hostile file names and `Content-Disposition` headers, size limits, tokens, and answers that try to send the browser to another site.
- A fuzz entry: `crates/sc-maker/tests/prop_maker.rs` (proptest, 2,000 cases each) asserts that anything accepted is MakerFoundry's, that names are plain, that a draft page is always on the request's site,
  that only an `sfs_` token is signed with, and that nothing panics.
- Each protection was removed in turn (31 single mutations, run by a script) and a test failed for all of them but four that are protected twice (an explicit check and the allow-list of characters:
  fragment, spaces/controls, backslash) and a size constant that had no test of its value (now tested). With both layers removed, the tests failed for fragment and spaces/controls; the backslash has three layers.
- The dialogs end to end with a recording host (`crates/ui-egui/src/maker/tests.rs`): nothing is sent before Send, the password and token are not kept, an ended sign-in goes back to sign-in, a draft page on
  another site is never opened. The desktop client against a server on this machine (`apps/vectorcraft/src/http.rs`): request as built, redirects not followed, answers capped, other requests never sent.

## Checked live and in a browser (2026-10-31)
- **Live services, with the owner's OK.** A throwaway Foundry account (`/v1/signup`, 100 credits), a tiny SVG published with the request shape the app builds (`POST /api/v1/designs`): `201`, a draft of the
  laser kind, format SVG (design 3 on MakerFoundry, left there: apps cannot delete); sign-out `200`. Also `GET` of design 1's file: `404` with MakerFoundry's sentence "That design isn't on MakerFoundry.",
  and CORS allowing `https://stitchwright.github.io` and exposing `Content-Disposition`; the account service answers the web app's preflight (`authorization`, `content-type`, `POST`).
  (The requests were sent with `curl` in the shape `sc-maker` builds; the desktop client's own HTTPS against the live services was not run.)
- **The real web build in Chromium** (`trunk build`, served locally, MakerFoundry and the account service mocked at the network edge): 20 checks for Open in ScreenFoundry and 18 for Publish, all passing:
  the page boots under its policy with no policy errors (the hook filled one script hash); a good link opens as `tiger-shirt.svg`, with exactly one request, for the checked address; `?open=` leaves the
  address and other parameters stay; `403`, `404` and `429` show MakerFoundry's own sentence; nine look-alike or foreign addresses (`.evil.com`, `@evil.com`, a port, a trailing dot, `http`, `..`, `%2e%2e`,
  another host, `javascript:`) make no request at all; a redirect is not followed (the other site is never asked); a `.dxf` file is refused; a hostile header name opens as a plain name; a declared 40 MB is
  refused; publishing signs in (POST to the account service), shows the Publish dialog before anything goes to MakerFoundry, sends a signed POST with the typed name, and opens the draft page in a new tab only
  when it is on MakerFoundry (an answer naming another site opens nothing; a `429` shows its sentence). The scripts were Playwright scripts kept outside the repository (this repository is Rust only); to repeat
  the check: `cd apps/vectorcraft-web && trunk build`, serve `dist/web`, load `/?open=<encoded address>` with MakerFoundry's address routed to a stand-in.

## Not checked
- **The hosted page.** `https://stitchwright.github.io/screenfoundry/` returns 404 until the repository is public and Pages is on; the CI step `cargo xtask web-csp dist/web/index.html --check` then runs on the
  real site build. The Chromium run used a local server.
- A real MakerFoundry design opened from its page (the button is off until it is switched on, and MakerFoundry's side is not deployed yet), Windows and macOS, and a software-rendered WebGL (not WebGPU) browser.
- The sign-in is not kept between runs (memory only), by design.

## For MakerFoundry's chat
The web app's address is `https://stitchwright.github.io/screenfoundry/` and it opens **SVG only**. MakerFoundry's addresses (`/api/v1/designs/{id}/file/{slug}.svg`, optionally `?t=<48 hex>`) pass every check
(`sc-maker` tests use the same shapes). The Publish dialog has a name field because the draft's title comes from the file name; the app string is `ScreenFoundry <version>`. Switch the button on once the
repository is public and Pages serves the app.

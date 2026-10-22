//! The job sheet: a letter-size PDF that travels with the films. It lists the job (size, resolution),
//! one row per screen in print order (colour swatch, name, role, mesh, coverage, file) and any
//! advice, so the press crew knows what each film is without opening it.

use crate::error::{Result, param};
use crate::pdfout::{self, Pdf};
use std::fmt::Write as _;

/// Most screens or advice lines a sheet takes; more is a mistake upstream, not a bigger sheet.
const MAX_ROWS: usize = 200;

/// One screen's row.
#[derive(Clone, Debug, PartialEq)]
pub struct SheetFilm {
    pub name: String,
    pub rgb: [u8; 3],
    /// `underbase` or `color`.
    pub role: String,
    pub mesh: f64,
    /// Share of the film that is ink, 0 to 1.
    pub coverage: f64,
    /// The film's file name, when it was written.
    pub file: Option<String>,
}

/// What the sheet says.
#[derive(Clone, Debug, PartialEq)]
pub struct JobSheet {
    pub title: String,
    pub notes: String,
    pub dpi: f64,
    pub width_px: usize,
    pub height_px: usize,
    pub films: Vec<SheetFilm>,
    pub advice: Vec<String>,
}

const PAGE: (f64, f64) = (612.0, 792.0);
const LEFT: f64 = 50.0;
const BOTTOM: f64 = 60.0;

/// A PDF string literal in WinAnsi: Latin-1 as is, anything else `?`, delimiters escaped.
fn literal(s: &str) -> String {
    let mut out = String::from("(");
    for c in s.chars() {
        match c {
            '(' | ')' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            c if (' '..='~').contains(&c) => out.push(c),
            c if ('\u{a0}'..='\u{ff}').contains(&c) => {
                let _ = write!(out, "\\{:03o}", c as u32);
            }
            _ => out.push('?'),
        }
    }
    out.push(')');
    out
}

/// At most `n` characters, ending in an ellipsis when cut.
fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    let mut t: String = s.chars().take(n.saturating_sub(3)).collect();
    t.push_str("...");
    t
}

/// Words onto lines of at most `n` characters; a word longer than that is cut.
fn wrap(s: &str, n: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    for word in s.split_whitespace() {
        let word = clip(word, n);
        if !cur.is_empty() && cur.chars().count() + 1 + word.chars().count() > n {
            lines.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(&word);
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

/// Accumulates the page contents and starts a new page when one fills.
struct Writer {
    pages: Vec<String>,
    cur: String,
    y: f64,
}

impl Writer {
    fn new() -> Self {
        Self { pages: Vec::new(), cur: String::new(), y: PAGE.1 - 60.0 }
    }
    fn room(&mut self, need: f64) {
        if self.y - need < BOTTOM {
            self.pages.push(std::mem::take(&mut self.cur));
            self.y = PAGE.1 - 60.0;
        }
    }
    fn text(&mut self, x: f64, size: f64, bold: bool, s: &str) {
        let font = if bold { "F2" } else { "F1" };
        let _ = writeln!(self.cur, "BT /{font} {size} Tf {x:.2} {:.2} Td {} Tj ET", self.y, literal(s));
    }
    fn swatch(&mut self, rgb: [u8; 3]) {
        let [r, g, b] = rgb.map(|c| f64::from(c) / 255.0);
        let _ = writeln!(
            self.cur,
            "{r:.3} {g:.3} {b:.3} rg {LEFT:.2} {:.2} 12 12 re f 0 0 0 RG 0.5 w {LEFT:.2} {:.2} 12 12 re S 0 g",
            self.y - 2.0,
            self.y - 2.0
        );
    }
    fn rule(&mut self) {
        let _ = writeln!(self.cur, "0.5 w {LEFT:.2} {:.2} m {:.2} {:.2} l S", self.y - 4.0, PAGE.0 - LEFT, self.y - 4.0);
    }
}

/// Renders the sheet. Fails only when it holds an absurd number of rows.
pub fn pdf(sheet: &JobSheet) -> Result<Vec<u8>> {
    if sheet.films.len() > MAX_ROWS || sheet.advice.len() > MAX_ROWS {
        return Err(param("jobSheet", format!("at most {MAX_ROWS} screens and advice lines")));
    }
    if !sheet.dpi.is_finite() || sheet.dpi <= 0.0 {
        return Err(param("dpi", "must be above 0"));
    }
    let mut w = Writer::new();
    w.text(LEFT, 18.0, true, &clip(&sheet.title, 60));
    w.y -= 22.0;
    let (wi, hi) = (sheet.width_px as f64 / sheet.dpi, sheet.height_px as f64 / sheet.dpi);
    let size = format!(
        "Film size {wi:.2} x {hi:.2} in ({:.0} x {:.0} mm) at {:.0} dpi, {} x {} px",
        wi * 25.4,
        hi * 25.4,
        sheet.dpi,
        sheet.width_px,
        sheet.height_px
    );
    w.text(LEFT, 10.0, false, &size);
    w.y -= 14.0;
    w.text(LEFT, 10.0, false, &format!("{} screens, in print order (first prints first)", sheet.films.len()));
    w.y -= 14.0;
    for line in wrap(&sheet.notes, 95) {
        w.room(14.0);
        w.text(LEFT, 10.0, false, &line);
        w.y -= 14.0;
    }
    w.y -= 8.0;
    let head = |w: &mut Writer| {
        for (x, h) in [(70.0, "Screen"), (250.0, "Role"), (310.0, "Mesh"), (365.0, "Coverage"), (430.0, "File")] {
            w.text(x, 9.0, true, h);
        }
        w.rule();
        w.y -= 18.0;
    };
    head(&mut w);
    for (i, f) in sheet.films.iter().enumerate() {
        if w.y - 18.0 < BOTTOM {
            w.room(f64::MAX / 4.0);
            head(&mut w);
        }
        w.swatch(f.rgb);
        w.text(70.0, 9.0, false, &clip(&format!("{}. {}", i + 1, f.name), 32));
        w.text(250.0, 9.0, false, &f.role);
        w.text(310.0, 9.0, false, &format!("{:.0}", f.mesh));
        w.text(365.0, 9.0, false, &format!("{:.1}%", f.coverage * 100.0));
        w.text(430.0, 9.0, false, &clip(f.file.as_deref().unwrap_or("-"), 28));
        w.y -= 18.0;
    }
    if !sheet.advice.is_empty() {
        w.y -= 8.0;
        w.room(30.0);
        w.text(LEFT, 11.0, true, "Advice");
        w.y -= 16.0;
        for a in &sheet.advice {
            for (k, line) in wrap(a, 92).iter().enumerate() {
                w.room(14.0);
                w.text(if k == 0 { LEFT } else { LEFT + 12.0 }, 9.0, false, line);
                w.y -= 13.0;
            }
        }
    }
    w.y = 40.0;
    w.text(LEFT, 8.0, false, "Made with ScreenFoundry");
    w.pages.push(std::mem::take(&mut w.cur));

    let mut pdf = Pdf::new();
    let catalog = pdf.reserve();
    let pages = pdf.reserve();
    let fonts = "/Font << /F1 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >> \
                 /F2 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >> >>";
    let mut kids = String::new();
    for content in &w.pages {
        let stream = pdf.add(&pdfout::stream("", content.as_bytes()));
        let page = pdf.add(
            format!("<< /Type /Page /Parent {pages} 0 R /MediaBox [0 0 {} {}] /Resources << {fonts} >> /Contents {stream} 0 R >>", PAGE.0, PAGE.1)
                .as_bytes(),
        );
        let _ = write!(kids, "{page} 0 R ");
    }
    pdf.set(catalog, format!("<< /Type /Catalog /Pages {pages} 0 R >>").as_bytes());
    pdf.set(pages, format!("<< /Type /Pages /Kids [{}] /Count {} >>", kids.trim_end(), w.pages.len()).as_bytes());
    pdf.finish(catalog)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sheet(n: usize) -> JobSheet {
        JobSheet {
            title: "Spring (tour) \\ 2026".into(),
            notes: "Cure at 160 C. Print light to dark.".into(),
            dpi: 600.0,
            width_px: 6000,
            height_px: 7500,
            films: (0..n)
                .map(|i| SheetFilm {
                    name: format!("Ink {}", i + 1),
                    rgb: [200, 30, 30],
                    role: if i == 0 { "underbase".into() } else { "color".into() },
                    mesh: 156.0,
                    coverage: 0.2345,
                    file: Some(format!("{:02}-ink.pdf", i + 1)),
                })
                .collect(),
            advice: vec!["Mesh 230 is too fine for a 45 lpi screen.".into()],
        }
    }

    fn text(pdf: &[u8]) -> String {
        String::from_utf8_lossy(pdf).into_owned()
    }

    #[test]
    fn lists_the_job_and_every_screen() {
        let t = text(&pdf(&sheet(3)).unwrap());
        assert!(t.starts_with("%PDF-1.4"));
        assert!(t.contains("Film size 10.00 x 12.50 in \\(254 x 318 mm\\) at 600 dpi, 6000 x 7500 px"), "{t}");
        // Delimiters in the title are escaped.
        assert!(t.contains("(Spring \\(tour\\) \\\\ 2026)"), "{t}");
        for i in 1..=3 {
            assert!(t.contains(&format!("({i}. Ink {i})")), "screen {i}");
            assert!(t.contains(&format!("({i:02}-ink.pdf)")));
        }
        assert!(t.contains("(23.4%)") || t.contains("(23.5%)"));
        assert!(t.contains("(underbase)") && t.contains("(Advice)") && t.contains("Mesh 230 is too fine"));
        assert!(t.contains("0.784 0.118 0.118 rg"), "the swatch is the ink colour");
        assert!(t.contains("/Count 1"));
    }

    #[test]
    fn many_screens_run_onto_more_pages_with_a_repeated_header() {
        let t = text(&pdf(&sheet(90)).unwrap());
        let pages = t.matches("/Type /Page ").count();
        assert!(pages >= 3, "{pages} pages");
        assert!(t.contains(&format!("/Count {pages}")));
        assert_eq!(t.matches("(Coverage)").count(), pages);
        assert!(t.contains("(90. Ink 90)"));
    }

    #[test]
    fn xref_points_at_every_object() {
        let bytes = pdf(&sheet(40)).unwrap();
        let t = text(&bytes);
        let start: usize = t[t.rfind("startxref\n").unwrap() + 10..].lines().next().unwrap().parse().unwrap();
        let table = String::from_utf8_lossy(&bytes[start..]).into_owned();
        let n: usize = table.lines().nth(1).unwrap().split(' ').nth(1).unwrap().parse().unwrap();
        for id in 1..n {
            let off: usize = table.lines().nth(id + 2).unwrap()[..10].parse().unwrap();
            assert!(bytes[off..].starts_with(format!("{id} 0 obj").as_bytes()), "object {id}");
        }
    }

    #[test]
    fn odd_text_and_bad_input_do_not_break_it() {
        let mut s = sheet(1);
        s.title = "Caf\u{e9} \u{2603} ".repeat(30);
        s.notes = "x".repeat(500);
        s.films[0].name = "n".repeat(200);
        s.films[0].file = None;
        let t = text(&pdf(&s).unwrap());
        assert!(t.contains("\\351"), "e-acute is octal-escaped");
        assert!(t.contains("?"), "the snowman is not WinAnsi");
        assert!(pdf(&sheet(MAX_ROWS + 1)).is_err());
        let mut bad = sheet(1);
        bad.dpi = f64::NAN;
        assert!(pdf(&bad).is_err());
        bad.dpi = 0.0;
        assert!(pdf(&bad).is_err());
    }
}

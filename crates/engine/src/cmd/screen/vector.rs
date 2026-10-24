//! Vector-first spot separation (SC-4): the document's own paths become the films.
//!
//! Where the raster commands render the art first and then split pixels, this one works on the
//! paths. Each filled shape (and each stroke, turned into its outline) belongs to an ink; going up
//! the stacking order a shape knocks out whatever of another ink lies under it, unless it overprints,
//! so an ink's plate is the union of its shapes with the knocked-out parts cut away. The underbase
//! and the traps are path operations too (an offset, then intersect and union), so edges stay exact
//! until the one step that makes pixels: each finished plate is drawn without smoothing at the film
//! resolution.
//!
//! What it cannot separate (gradients, patterns, placed images, meshes, live effects that need
//! expanding) is left out and listed in the result; opacity, blend modes, masks and effects are
//! ignored (every shape prints solid) and counted.

use sc_core::Bitmap1;
use sc_core::color::{delta_e_sq, srgb_to_lab};
use sc_core::job::{Film, Ink, Role};
use sc_core::marks::MarkSpec;
use serde_json::{Value, json};
use vectorcraft_color::{BlendMode, Color, Paint};
use vectorcraft_doc::appearance::AppearanceItem;
use vectorcraft_doc::{Appearance, Document, Node, NodeKind};
use vectorcraft_geom::{FillRule, PathData, Rect, shapes};
use vectorcraft_pathops::{BoolOp, Join, boolean, offset_path, unite_all};
use vectorcraft_render::{AntiAlias, RenderOptions, Renderer};

use super::*;
use crate::cmd::pathops::node_path;

/// Most shapes one job separates: knocking out is pairwise, so the work grows with the square.
const MAX_SHAPES: usize = 10_000;

/// Most tinted or gradient shapes one job screens: each is drawn and screened at the film size.
const MAX_SCREENED: usize = 64;

/// Most skipped objects listed in the result (the rest are counted).
const MAX_LISTED: usize = 50;

/// Inks at least this light (Lab L*) get no underbase: they are the underbase's own colour.
const WHITE_L: f64 = 95.0;

const MITER_LIMIT: f64 = 4.0;

/// How a shape prints: solid, or screened into dots at a tone.
#[derive(Clone)]
enum Tone {
    Solid,
    /// A spot tint below 100%: this share of the ink, as dots.
    Tint(f64),
    /// A gradient: the tone follows the colours the paint draws.
    Painted(Paint),
}

/// One painted shape of the art, in stacking order.
struct Raw {
    path: PathData,
    rule: FillRule,
    rgb: [u8; 3],
    /// The spot swatch the colour is linked to.
    spot: Option<String>,
    overprint: bool,
    tone: Tone,
    /// The top-level layer the shape is on (sublayers count as their layer).
    layer: usize,
    /// The object set the shape belongs to: its outermost group, else its layer (an index into
    /// `Collected::set_names`).
    set: usize,
}

#[derive(Default)]
struct Collected {
    /// The opacity of the groups being walked, multiplied.
    opacity: f64,
    /// Inside a group whose blend mode overprints.
    blend_overprint: bool,
    /// Whether tints and gradients are screened (else tints print solid and gradients are skipped).
    tones_on: bool,
    shapes: Vec<Raw>,
    /// The top-level layer being walked, and the name of each layer.
    layer: usize,
    layer_names: Vec<String>,
    /// The name of each object set, and the set being walked (None outside any group).
    set_names: Vec<String>,
    group: Option<usize>,
    /// The set of each layer's loose shapes (not in a group).
    layer_sets: Vec<(usize, usize)>,
    /// (object, why) for what could not be separated.
    skipped: Vec<(String, String)>,
    skipped_total: usize,
    /// Objects, fills and strokes whose opacity, blend mode, mask or effects were ignored.
    transparent: usize,
    /// Spot tints below 100% (they print solid).
    tinted: usize,
    /// Text objects with more than one style (they print in the first one's colours).
    mixed_text: usize,
}

impl Collected {
    fn skip(&mut self, n: &Node, why: &str) {
        self.skipped_total += 1;
        if self.skipped.len() < MAX_LISTED {
            let name = n.name.clone().unwrap_or_else(|| format!("object {}", n.id.0));
            self.skipped.push((name, why.to_string()));
        }
    }
}

/// Multiply and Darken print a shape over what is below it, as overprint does (the usual advice
/// for spot colours is overprint or Darken, not Multiply, which compounds when shapes stack).
fn overprints(b: BlendMode) -> bool {
    matches!(b, BlendMode::Multiply | BlendMode::Darken)
}

/// A blend mode that cannot be separated (Normal and the overprint-like ones can).
fn blend_ignored(b: BlendMode) -> bool {
    b != BlendMode::Normal && !overprints(b)
}

/// Whether `n` has transparency that cannot be screened: with `opacity_ok` plain opacity does not
/// count (a solid fill with opacity is screened as dots of that share).
fn is_transparent(n: &Node, opacity_ok: bool) -> bool {
    (n.opacity < 1.0 && !opacity_ok) || blend_ignored(n.blend) || n.extras().mask.is_some() || !n.appearance.effects.is_empty()
}

fn collect(doc: &Document, tones_on: bool) -> Collected {
    let mut out = Collected { tones_on, opacity: 1.0, ..Default::default() };
    for (i, l) in doc.layers.iter().enumerate() {
        out.layer = i;
        out.layer_names.push(l.name.clone().unwrap_or_else(|| format!("Layer {}", i + 1)));
        walk(doc, l, &[], &mut out);
    }
    out
}

type Clips<'a> = &'a [(PathData, FillRule)];

fn walk_children(doc: &Document, children: &[std::sync::Arc<Node>], clip: bool, clips: Clips, out: &mut Collected) {
    // With a clip the first (bottom-most) child is the clipping path, applied to the rest.
    let first = children.first().filter(|_| clip).and_then(|c| node_path(c));
    match first {
        Some((path, rule)) => {
            let mut inner: Vec<(PathData, FillRule)> = clips.to_vec();
            inner.push((path, rule));
            for c in children.iter().skip(1) {
                walk(doc, c, &inner, out);
            }
        }
        None => {
            for c in children.iter().skip(usize::from(clip && !children.is_empty())) {
                walk(doc, c, clips, out);
            }
        }
    }
}

fn walk(doc: &Document, n: &Node, clips: Clips, out: &mut Collected) {
    if !n.visible {
        return;
    }
    match &n.kind {
        NodeKind::Layer { template, printable, children, clip, .. } => {
            if *template || !*printable {
                return;
            }
            walk_children(doc, children, *clip, clips, out);
        }
        NodeKind::Group { children, clip } => {
            out.transparent += usize::from(is_transparent(n, out.tones_on));
            let outer_opacity = out.opacity;
            let outer_blend = out.blend_overprint;
            out.blend_overprint |= overprints(n.blend);
            out.opacity *= f64::from(n.opacity).clamp(0.0, 1.0);
            // The outermost group is the object set; groups inside it belong to it.
            let outer = out.group.is_none();
            if outer {
                out.set_names.push(n.name.clone().unwrap_or_else(|| format!("Group {}", out.set_names.len() + 1)));
                out.group = Some(out.set_names.len() - 1);
            }
            walk_children(doc, children, *clip, clips, out);
            out.opacity = outer_opacity;
            out.blend_overprint = outer_blend;
            if outer {
                out.group = None;
            }
        }
        NodeKind::Path { guide, clipping, .. } => {
            if !*guide && !*clipping {
                leaf(doc, n, clips, out);
            }
        }
        NodeKind::Compound { .. } | NodeKind::Text(_) => leaf(doc, n, clips, out),
        NodeKind::Image(_) | NodeKind::PlacedDocument(_) => out.skip(n, "a picture: separate it with the raster commands"),
        NodeKind::Mesh(_) => out.skip(n, "a gradient mesh"),
        NodeKind::SymbolInstance { .. } => out.skip(n, "a symbol instance: break the link first"),
        _ => out.skip(n, "live art: expand it first"),
    }
}

fn rgb8(doc: &Document, color: &Color) -> [u8; 3] {
    let v = vectorcraft_color::cms::active().display_rgb(&doc.linked_color(*color, true));
    v.map(|c| (c * 255.0).round().clamp(0.0, 255.0) as u8)
}

fn leaf(doc: &Document, n: &Node, clips: Clips, out: &mut Collected) {
    let Some((path, rule)) = node_path(n) else { return };
    out.transparent += usize::from(is_transparent(n, out.tones_on));
    let node_alpha = out.opacity * f64::from(n.opacity).clamp(0.0, 1.0);
    let owned;
    let appearance: &Appearance = match &n.kind {
        NodeKind::Text(t) => {
            out.mixed_text += usize::from(t.runs.len() > 1);
            owned = t.first_style().appearance();
            &owned
        }
        _ => &n.appearance,
    };
    for item in &appearance.items {
        match item {
            AppearanceItem::Fill(f) => {
                if f.visible && !f.paint.is_none() {
                    out.transparent += usize::from((f.opacity < 1.0 && !out.tones_on) || blend_ignored(f.blend) || !f.effects.is_empty());
                    let alpha = node_alpha * f64::from(f.opacity).clamp(0.0, 1.0);
                    let over = f.overprint || overprints(f.blend) || overprints(n.blend) || out.blend_overprint;
                    add(doc, n, &path, rule, &f.paint, (over, alpha), clips, out);
                }
            }
            AppearanceItem::Stroke(st) => {
                if !st.visible || st.paint.is_none() || st.width <= 0.0 {
                    continue;
                }
                if st.brush.is_some() {
                    out.skip(n, "a brush stroke: Outline Stroke it first");
                    continue;
                }
                out.transparent += usize::from((st.opacity < 1.0 && !out.tones_on) || blend_ignored(st.blend) || !st.effects.is_empty());
                let alpha = node_alpha * f64::from(st.opacity).clamp(0.0, 1.0);
                let outline = vectorcraft_render::effects::stroke::outline_region(&path, rule, st);
                let over = st.overprint || overprints(st.blend) || overprints(n.blend) || out.blend_overprint;
                add(doc, n, &outline, FillRule::NonZero, &st.paint, (over, alpha), clips, out);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn add(
    doc: &Document,
    n: &Node,
    path: &PathData,
    rule: FillRule,
    paint: &Paint,
    (mut overprint, alpha): (bool, f64),
    clips: Clips,
    out: &mut Collected,
) {
    let (rgb, spot, tone) = match paint {
        Paint::Solid { color, swatch, tint } => {
            let spot = swatch.as_ref().filter(|s| doc.swatch(s).is_some_and(|sw| sw.spot)).cloned();
            let tinted = spot.is_some() && *tint < 1.0;
            // Opacity prints as a share of the ink, like a tint.
            let share = if spot.is_some() { f64::from(*tint) } else { 1.0 } * alpha;
            if (tinted || alpha < 1.0) && out.tones_on {
                // The ink is the swatch's own colour; the dots carry the tint and the opacity.
                let full = spot
                    .as_ref()
                    .and_then(|s| doc.swatch(s))
                    .and_then(|sw| if let Paint::Solid { color, .. } = &sw.paint { Some(*color) } else { None })
                    .unwrap_or(*color);
                // A see-through shape lets what is below show: it does not knock it out.
                overprint |= alpha < 1.0;
                (rgb8(doc, &full), spot, Tone::Tint(share.clamp(0.0, 1.0)))
            } else {
                out.tinted += usize::from(tinted);
                (rgb8(doc, color), spot, Tone::Solid)
            }
        }
        Paint::Gradient(g) if out.tones_on => {
            // The ink is the darkest colour of the gradient: the tone is how far each point is from white towards it.
            let darkest = g.gradient.stops.iter().map(|st| rgb8(doc, &st.color)).min_by(|a, b| srgb_to_lab(*a)[0].total_cmp(&srgb_to_lab(*b)[0]));
            let Some(rgb) = darkest else {
                out.skip(n, "a gradient with no stops");
                return;
            };
            (rgb, None, Tone::Painted(paint.clone()))
        }
        _ => {
            out.skip(n, "a gradient or pattern: give it a solid colour");
            return;
        }
    };
    let (mut path, mut rule) = (path.clone(), rule);
    for (clip, clip_rule) in clips {
        path = boolean(&path, rule, clip, *clip_rule, BoolOp::Intersect);
        rule = FillRule::NonZero;
    }
    if !path.is_empty() {
        let set = match out.group {
            Some(g) => g,
            // Loose shapes share one set per layer, named after the layer.
            None => match out.layer_sets.iter().find(|(l, _)| *l == out.layer) {
                Some(&(_, set)) => set,
                None => {
                    out.set_names.push(out.layer_names.get(out.layer).cloned().unwrap_or_else(|| format!("Layer {}", out.layer + 1)));
                    out.layer_sets.push((out.layer, out.set_names.len() - 1));
                    out.set_names.len() - 1
                }
            },
        };
        out.shapes.push(Raw { path, rule, rgb, spot, overprint, tone, layer: out.layer, set });
    }
}

/// A shape put on an ink.
pub(super) struct Shape {
    pub path: PathData,
    pub rule: FillRule,
    pub ink: usize,
    pub overprint: bool,
    /// Printed as dots at a tone rather than solid: kept out of the solid plate.
    pub screened: bool,
}

fn overlaps(a: Rect, b: Rect) -> bool {
    a.x0 <= b.x1 && b.x0 <= a.x1 && a.y0 <= b.y1 && b.y0 <= a.y1
}

/// One plate per ink (`inks` of them) from shapes in stacking order, bottom first. A shape knocks
/// out the shapes of other inks below it where they overlap, unless it overprints; so each plate
/// holds only what shows (what the ink prints) plus what is printed over.
#[cfg(test)]
pub(super) fn compose(shapes: &[Shape], inks: usize) -> Vec<PathData> {
    compose_parts(shapes, inks).0
}

/// [`compose`] with the screened shapes kept apart: the solid plates, and for each screened
/// shape (by index) the part of it that shows.
pub(super) fn compose_parts(shapes: &[Shape], inks: usize) -> (Vec<PathData>, Vec<(usize, PathData)>) {
    let mut screened_parts: Vec<(usize, PathData)> = Vec::new();
    let boxes: Vec<Option<Rect>> = shapes.iter().map(|s| s.path.bounds()).collect();
    let mut parts: Vec<Vec<(PathData, FillRule)>> = vec![Vec::new(); inks];
    for (i, s) in shapes.iter().enumerate() {
        let Some(mine) = boxes.get(i).copied().flatten() else { continue };
        let above: Vec<(&PathData, FillRule)> = shapes
            .iter()
            .zip(&boxes)
            .skip(i + 1)
            .filter(|(t, b)| t.ink != s.ink && !t.overprint && b.is_some_and(|b| overlaps(b, mine)))
            .map(|(t, _)| (&t.path, t.rule))
            .collect();
        let visible = if above.is_empty() {
            (s.path.clone(), s.rule)
        } else {
            (boolean(&s.path, s.rule, &unite_all(&above), FillRule::NonZero, BoolOp::Difference), FillRule::NonZero)
        };
        if visible.0.is_empty() {
            continue;
        }
        if s.screened {
            screened_parts.push((i, unite_all(&[(&visible.0, visible.1)])));
        } else if let Some(p) = parts.get_mut(s.ink) {
            p.push(visible);
        }
    }
    let plates = parts
        .iter()
        .map(|p| {
            let refs: Vec<(&PathData, FillRule)> = p.iter().map(|(path, rule)| (path, *rule)).collect();
            unite_all(&refs)
        })
        .collect();
    (plates, screened_parts)
}

/// Trap `plates` (in print order): each grows by `trap` (document units) but only where a plate
/// that prints after it lies, so the light colour tucks under the dark one and its free edges stay
/// where the art put them.
pub(super) fn trap_under(plates: &[PathData], trap: f64) -> Vec<PathData> {
    if trap <= 0.0 || !trap.is_finite() {
        return plates.to_vec();
    }
    plates
        .iter()
        .enumerate()
        .map(|(k, p)| {
            let later: Vec<(&PathData, FillRule)> = plates.iter().skip(k + 1).filter(|q| !q.is_empty()).map(|q| (q, FillRule::NonZero)).collect();
            if p.is_empty() || later.is_empty() {
                return p.clone();
            }
            let lap =
                boolean(&offset_path(p, trap, Join::Round, MITER_LIMIT), FillRule::NonZero, &unite_all(&later), FillRule::NonZero, BoolOp::Intersect);
            unite_all(&[(p, FillRule::NonZero), (&lap, FillRule::NonZero)])
        })
        .collect()
}

/// Like [`trap_under`] with a width of its own for each pair: plate `k` grows by `width(k, j)` under
/// each later plate `j` (0 or less: not at all there).
pub(super) fn trap_pairs(plates: &[PathData], width: impl Fn(usize, usize) -> f64) -> Vec<PathData> {
    plates
        .iter()
        .enumerate()
        .map(|(k, p)| {
            if p.is_empty() {
                return p.clone();
            }
            let mut laps: Vec<PathData> = Vec::new();
            for (j, q) in plates.iter().enumerate().skip(k + 1) {
                let w = width(k, j);
                if q.is_empty() || w <= 0.0 || !w.is_finite() {
                    continue;
                }
                let lap = boolean(&offset_path(p, w, Join::Round, MITER_LIMIT), FillRule::NonZero, q, FillRule::NonZero, BoolOp::Intersect);
                if !lap.is_empty() {
                    laps.push(lap);
                }
            }
            let mut all: Vec<(&PathData, FillRule)> = vec![(p, FillRule::NonZero)];
            all.extend(laps.iter().map(|l| (l, FillRule::NonZero)));
            unite_all(&all)
        })
        .collect()
}

/// The traps asked for by `pairs: [{spread, under, mm}]`, as (ink that spreads, ink it tucks
/// under, width in mm) by ink index.
fn pair_traps(p: &Value, inks: &[Ink]) -> Result<Vec<(usize, usize, f64)>> {
    const C: &str = "screen.separateVector";
    let list = match p.get("pairs") {
        None | Some(Value::Null) => return Ok(Vec::new()),
        Some(Value::Array(a)) if a.len() <= 64 => a,
        Some(_) => return Err(bad(C, "`pairs` is a list of up to 64 {spread, under, mm}")),
    };
    let find = |v: Option<&Value>, key: &str| -> Result<usize> {
        let name = v.and_then(Value::as_str).ok_or_else(|| bad(C, format!("each pair needs `{key}`, an ink name")))?;
        inks.iter().position(|i| i.name.eq_ignore_ascii_case(name)).ok_or_else(|| bad(C, format!("no ink named {name:?} for a trap pair")))
    };
    list.iter().map(|e| Ok((find(e.get("spread"), "spread")?, find(e.get("under"), "under")?, num(e, C, "mm", 0.25, 0.0..=5.0)?))).collect()
}

/// A document whose one artboard is `region` and whose one object is `plate` filled with `ink`.
fn plate_document(plate: &PathData, region: Rect, paint: Paint) -> Result<Document> {
    const C: &str = "screen.separateVector";
    let mut doc = Document::new(region.width().max(1.0), region.height().max(1.0));
    if let Some(a) = doc.artboards.first_mut() {
        a.rect = region;
    }
    let layer = doc.layers.first().map(|l| l.id).ok_or_else(|| bad(C, "internal: no layer"))?;
    let id = doc.alloc_id();
    let node = Node::path(id, plate.clone(), Appearance::basic(paint, Paint::None, 0.0));
    doc.insert(Some(layer), 0, node).map_err(|e| bad(C, e.to_string()))?;
    Ok(doc)
}

/// `plate` as a one-page vector PDF the size of the artboard: black ink (the film's own K) on nothing.
fn plate_pdf(plate: &PathData, region: Rect) -> Result<Vec<u8>> {
    const C: &str = "screen.separateVector";
    let doc = plate_document(plate, region, Paint::solid(Color::Gray { k: 1.0 }))?;
    crate::cmd::rasterfx::export_pdf(&doc, &vectorcraft_pdf::PdfOptions::default()).map_err(|e| bad(C, e.to_string()))
}

/// `plate` as an SVG the size of the artboard: black ink on nothing.
fn plate_svg(plate: &PathData, region: Rect) -> Result<Vec<u8>> {
    let doc = plate_document(plate, region, Paint::solid(Color::Gray { k: 1.0 }))?;
    Ok(vectorcraft_svg::export(&doc, &vectorcraft_svg::ExportOptions::default()).into_bytes())
}

/// Draw `plate` as black on nothing, without smoothing, `w` × `h` pixels over `region`, and
/// threshold it into film.
fn rasterize(r: &mut Renderer, plate: &PathData, region: Rect, scale: f64, (w, h): (u32, u32)) -> Result<Bitmap1> {
    const C: &str = "screen.separateVector";
    let doc = plate_document(plate, region, Paint::solid(Color::BLACK))?;
    let view = vectorcraft_geom::Affine::scale(scale) * vectorcraft_geom::Affine::translate((-region.x0, -region.y0));
    let opts = RenderOptions { background: None, artboards: false, skip_templates: true, anti_alias: AntiAlias::None, ..Default::default() };
    let img = r.render(&doc, w, h, view, &opts);
    let bits: Vec<bool> = img.pixels.chunks_exact(4).map(|px| px.get(3).copied().unwrap_or(0) >= 128).collect();
    Bitmap1::from_bits(w as usize, h as usize, bits).map_err(sc(C))
}

/// The screening for tints and gradients: the dots of a halftone at the job's ruling.
struct ToneScreen {
    opts: HalftoneOptions,
    scatter: Option<Scatter>,
    curve: Option<sc_core::curve::ToneCurve>,
}

/// `path` filled with `paint`, drawn without smoothing into `size` pixels over `region`.
fn render_painted(r: &mut Renderer, path: &PathData, paint: &Paint, region: Rect, scale: f64, (w, h): (u32, u32)) -> Result<sc_core::Rgba8> {
    const C: &str = "screen.separateVector";
    let doc = plate_document(path, region, paint.clone())?;
    let view = vectorcraft_geom::Affine::scale(scale) * vectorcraft_geom::Affine::translate((-region.x0, -region.y0));
    let opts = RenderOptions { background: None, artboards: false, skip_templates: true, anti_alias: AntiAlias::None, ..Default::default() };
    let img = r.render(&doc, w, h, view, &opts);
    sc_core::Rgba8::new(w as usize, h as usize, img.to_straight()).map_err(sc(C))
}

/// The dots of one screened shape: its visible `part`, screened at the tone it carries (a tint's
/// share, or what its gradient draws) with the ink at `angle`.
#[allow(clippy::too_many_arguments)]
fn screened_part(
    r: &mut Renderer,
    shape: &Shape,
    tone: &Tone,
    part: &PathData,
    ink_rgb: [u8; 3],
    angle: f64,
    screen: &ToneScreen,
    (region, scale, size): (Rect, f64, (u32, u32)),
) -> Result<Bitmap1> {
    const C: &str = "screen.separateVector";
    let mask = rasterize(r, part, region, scale, size)?;
    let (w, h) = (size.0 as usize, size.1 as usize);
    let plate = match tone {
        Tone::Tint(t) => {
            let v = (t.clamp(0.0, 1.0) * 255.0).round() as u8;
            let data: Vec<u8> = (0..w * h).map(|i| if mask.get(i % w, i / w) { v } else { 0 }).collect();
            sc_core::Gray8::new(w, h, data).map_err(sc(C))?
        }
        Tone::Painted(paint) => {
            let img = render_painted(r, &shape.path, paint, region, scale, size)?;
            sc_core::separate::tone_plate(&img, ink_rgb).map_err(sc(C))?
        }
        Tone::Solid => return Ok(mask),
    };
    let plate = match &screen.curve {
        Some(curve) => curve.compensate(&plate).map_err(sc(C))?,
        None => plate,
    };
    let opts = HalftoneOptions { angle_deg: angle, ..screen.opts };
    let dots = screen_plate(&plate, &opts, &screen.scatter).map_err(sc(C))?;
    let bits: Vec<bool> = (0..w * h).map(|i| mask.get(i % w, i / w) && dots.get(i % w, i / w)).collect();
    Bitmap1::from_bits(w, h, bits).map_err(sc(C))
}

/// The inks of the job and the ink of each shape.
fn assign_inks(p: &Value, raws: &[Raw], layer_names: &[String], set_names: &[String]) -> Result<(Vec<Ink>, Vec<usize>)> {
    const C: &str = "screen.separateVector";
    let mut rgbs: Vec<[u8; 3]> = Vec::new();
    let mut names: Vec<Option<String>> = Vec::new();
    let mut meshes: Vec<f64> = Vec::new();
    let mut given = false;
    match p.get("inks") {
        None | Some(Value::Null) => {}
        Some(Value::Array(a)) => {
            if a.is_empty() || a.len() > MAX_INKS {
                return Err(bad(C, format!("`inks` holds 1 to {MAX_INKS} inks")));
            }
            given = true;
            for i in a {
                rgbs.push(color(C, "inks[].color", i.get("color"), [0, 0, 0])?);
                names.push(Some(name_of(i.get("name"), String::new())).filter(|n| !n.is_empty()));
                meshes.push(num(i, C, "mesh", 156.0, 1.0..=1000.0)?);
            }
        }
        Some(_) => return Err(bad(C, "`inks` is a list of {name?, color, mesh?}")),
    }
    let (by_layer, by_set) = match str_param(p, "inkBy").unwrap_or("color") {
        "color" => (false, false),
        "layer" => (true, false),
        "group" => (true, true),
        _ => return Err(bad(C, "`inkBy` is color, layer or group")),
    };
    // By layer or group the given inks only set the colour and mesh of the one they are named for.
    let overrides = by_layer.then(|| (std::mem::take(&mut rgbs), std::mem::take(&mut names), std::mem::take(&mut meshes)));
    // The ink of each shape: by layer its layer's ink; else with given inks a spot swatch takes the
    // ink of its name and otherwise the nearest colour; without, every distinct colour or spot
    // swatch becomes an ink.
    let mut of_shape = Vec::with_capacity(raws.len());
    let mut keys: Vec<(Option<String>, [u8; 3])> = Vec::new();
    let mut layers_seen: Vec<usize> = Vec::new();
    for r in raws {
        let ink = if by_layer {
            let unit = if by_set { r.set } else { r.layer };
            match layers_seen.iter().position(|&l| l == unit) {
                Some(i) => Some(i),
                None => {
                    if layers_seen.len() >= MAX_INKS {
                        return Err(bad(C, format!("the art has shapes in more than {MAX_INKS} layers or groups: merge them or use `inkBy` color")));
                    }
                    layers_seen.push(unit);
                    let name =
                        if by_set { set_names.get(unit) } else { layer_names.get(unit) }.cloned().unwrap_or_else(|| format!("Layer {}", unit + 1));
                    let given_at = overrides
                        .as_ref()
                        .and_then(|(_, g_names, _)| g_names.iter().position(|n| n.as_deref().is_some_and(|n| n.eq_ignore_ascii_case(&name))));
                    let (g_rgb, g_mesh) = match (&overrides, given_at) {
                        (Some((rgbs, _, meshes)), Some(i)) => (rgbs.get(i).copied(), meshes.get(i).copied()),
                        _ => (None, None),
                    };
                    rgbs.push(g_rgb.unwrap_or(r.rgb));
                    names.push(Some(name));
                    meshes.push(g_mesh.unwrap_or(156.0));
                    Some(layers_seen.len() - 1)
                }
            }
        } else if given {
            let by_name = r.spot.as_ref().and_then(|s| names.iter().position(|n| n.as_deref().is_some_and(|n| n.eq_ignore_ascii_case(s))));
            by_name.or_else(|| {
                let lab = srgb_to_lab(r.rgb);
                rgbs.iter()
                    .enumerate()
                    .min_by(|a, b| delta_e_sq(lab, srgb_to_lab(*a.1)).total_cmp(&delta_e_sq(lab, srgb_to_lab(*b.1))))
                    .map(|(i, _)| i)
            })
        } else {
            let key = (r.spot.clone(), if r.spot.is_some() { [0; 3] } else { r.rgb });
            match keys.iter().position(|k| *k == key) {
                Some(i) => Some(i),
                None => {
                    if keys.len() >= MAX_INKS {
                        return Err(bad(C, format!("the art has more than {MAX_INKS} colours: give `inks` to say which to print")));
                    }
                    keys.push(key);
                    rgbs.push(r.rgb);
                    names.push(r.spot.clone());
                    meshes.push(156.0);
                    Some(keys.len() - 1)
                }
            }
        };
        of_shape.push(ink.ok_or_else(|| bad(C, "no ink to put a shape on"))?);
    }
    if rgbs.is_empty() {
        return Err(bad(C, "the artwork has no solid shapes to separate"));
    }
    snap_inks(C, p, &mut rgbs, &mut names)?;
    let inks = rgbs
        .iter()
        .enumerate()
        .map(|(i, &rgb)| Ink {
            name: names.get(i).cloned().flatten().unwrap_or_else(|| format!("Ink {}", i + 1)),
            rgb,
            mesh: meshes.get(i).copied().unwrap_or(156.0),
        })
        .collect();
    Ok((inks, of_shape))
}

/// The four registration marks as paths, in document points: the layout of the raster films'
/// marks (a crosshair in a circle at each corner, outside the art) at the size [`MarkSpec`] gives
/// for `dpi`, drawn around `art`. Also the region that holds the art and the marks, grown by the
/// same margin as a raster film, so a vector plate and a raster film line up.
fn vector_marks(art: Rect, dpi: f64) -> Result<(PathData, Rect)> {
    const C: &str = "screen.separateVector";
    let m = MarkSpec::for_dpi(dpi).map_err(sc(C))?;
    let k = 72.0 / dpi;
    let (size, line, gap) = (m.size_px as f64 * k, m.line_px as f64 * k, m.gap_px as f64 * k);
    let (r, off, margin) = (size / 2.0, gap + size / 2.0, gap + size + k);
    let inner = (r - line).max(0.0);
    let square = |cx: f64, cy: f64, hw: f64, hh: f64| shapes::rectangle(Rect::new(cx - hw, cy - hh, cx + hw, cy + hh));
    let mut parts: Vec<(PathData, FillRule)> = Vec::new();
    for (cx, cy) in [(art.x0 - off, art.y0 - off), (art.x1 + off, art.y0 - off), (art.x1 + off, art.y1 + off), (art.x0 - off, art.y1 + off)] {
        // The ring is the outer circle with the inner one cut out (even-odd).
        let mut ring = shapes::ellipse(Rect::new(cx - r, cy - r, cx + r, cy + r));
        if inner > 0.0 {
            ring.subpaths.extend(shapes::ellipse(Rect::new(cx - inner, cy - inner, cx + inner, cy + inner)).subpaths);
        }
        parts.push((ring, FillRule::EvenOdd));
        parts.push((square(cx, cy, r, line / 2.0), FillRule::NonZero));
        parts.push((square(cx, cy, line / 2.0, r), FillRule::NonZero));
    }
    let refs: Vec<(&PathData, FillRule)> = parts.iter().map(|(p, rule)| (p, *rule)).collect();
    Ok((unite_all(&refs), Rect::new(art.x0 - margin, art.y0 - margin, art.x1 + margin, art.y1 + margin)))
}

/// Give each film entry in `films` its plate as a vector PDF (`vector`): a file in `dir` when
/// there is one, else base64.
///
/// With `marks` (the marks and the larger region that holds them, from [`vector_marks`]) the page
/// grows to hold them and every plate carries them.
fn attach_vector_pdfs(
    films: &mut [Value],
    paths: &[PathData],
    region: Rect,
    marks: Option<&(PathData, Rect)>,
    dir: Option<&str>,
    format: &str,
) -> Result<()> {
    for (i, (entry, path)) in films.iter_mut().zip(paths).enumerate() {
        let (shape, page) = match marks {
            Some((mk, framed)) => (unite_all(&[(path, FillRule::NonZero), (mk, FillRule::NonZero)]), *framed),
            None => (path.clone(), region),
        };
        let bytes = if format == "svg" { plate_svg(&shape, page)? } else { plate_pdf(&shape, page)? };
        let mut v = json!({ "format": format });
        match dir {
            Some(dir) => {
                let role = entry["role"].as_str().unwrap_or("color").to_string();
                let name = entry["name"].as_str().unwrap_or("film").to_string();
                let file = format!("{}/{:02}-{}-{}.vector.{format}", dir.trim_end_matches(['/', '\\']), i + 1, role, slug(&name));
                crate::cmd::fileio::write_file(&file, &bytes)?;
                v["path"] = json!(file);
            }
            None => v["dataBase64"] = json!(vectorcraft_format::base64_encode(&bytes)),
        }
        entry["vector"] = v;
    }
    Ok(())
}

/// Add one layer per plate to the open document (one undo step), holding the plate's paths filled
/// with the ink's colour, named after the ink. Returns the new layers' ids.
fn plates_into_layers(s: &mut Session, plates: Vec<(String, [u8; 3], PathData)>) -> Result<Vec<u64>> {
    s.edit("Plates to Layers", |d, _| {
        let mut ids = Vec::with_capacity(plates.len());
        for (name, rgb, path) in plates {
            let layer_id = d.alloc_id();
            let colour = vectorcraft_doc::LayerColor::Preset((d.layers.len() % vectorcraft_doc::LAYER_COLORS.len()) as u8);
            d.layers.push(std::sync::Arc::new(Node::layer(layer_id, &format!("Plate: {name}"), colour)));
            if !path.is_empty() {
                let id = d.alloc_id();
                let fill = Color::rgb(f32::from(rgb[0]) / 255.0, f32::from(rgb[1]) / 255.0, f32::from(rgb[2]) / 255.0);
                let mut node = Node::path(id, path, Appearance::basic(Paint::solid(fill), Paint::None, 0.0));
                node.name = Some(name.clone());
                d.insert(Some(layer_id), 0, node).map_err(|e| bad("screen.separateVector", e.to_string()))?;
            }
            ids.push(layer_id.0);
        }
        Ok(ids)
    })
}

/// `screen.separateVector`: the open document's paths as one film per ink.
pub(super) fn separate_vector(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "screen.separateVector";
    if !matches!(p.get("source"), None | Some(Value::Null)) {
        return Err(bad(C, "this separates the open document's art: use screen.separate for an image file"));
    }
    let dpi = num(p, C, "dpi", DEFAULT_DPI, 25.0..=2400.0)?;
    let trap_mm = num(p, C, "trapMm", 0.25, 0.0..=5.0)?;
    let marks = bool_or(p, "marks", false);
    // `vectorPdf: true` and `vectorSvg: true` each add the plates as paths in that format (PDF wins).
    let vector_format = if bool_or(p, "vectorPdf", false) {
        Some("pdf")
    } else if bool_or(p, "vectorSvg", false) {
        Some("svg")
    } else {
        None
    };
    let fmt = FilmFormat::parse(C, p)?;
    // Lengths are millimetres in the parameters and points in the document.
    let pt = |mm: f64| mm / 25.4 * 72.0;
    let underbase = underbase_of(p, C, pt)?;

    let st = s.doc().map_err(|_| bad(C, "no document is open"))?;
    // Symbol instances are separated through a copy with their links broken.
    let mut has_symbols = false;
    for l in &st.doc.layers {
        l.walk(&mut |c| has_symbols |= matches!(c.kind, NodeKind::SymbolInstance { .. }));
    }
    let broken;
    let doc: &Document = if has_symbols {
        let mut copy = (*st.doc).clone();
        crate::cmd::brushsym::break_all_symbol_links(&mut copy);
        broken = copy;
        &broken
    } else {
        &st.doc
    };
    let region = doc.artboards.first().ok_or_else(|| bad(C, "the document has no artboard"))?.rect;
    let scale = dpi / 72.0;
    let size = vectorcraft_render::raster_size(region, scale).map_err(|e| bad(C, e))?;
    let tones_on = bool_or(p, "tones", true);
    let collected = collect(doc, tones_on);
    if collected.shapes.len() > MAX_SHAPES {
        return Err(bad(C, format!("the art has {} shapes; at most {MAX_SHAPES} can be separated at once", collected.shapes.len())));
    }
    let (inks, of_shape) = assign_inks(p, &collected.shapes, &collected.layer_names, &collected.set_names)?;
    let tones: Vec<Tone> = collected.shapes.iter().map(|r| r.tone.clone()).collect();
    let shapes: Vec<Shape> = collected
        .shapes
        .into_iter()
        .zip(&of_shape)
        .map(|(r, &ink)| Shape { screened: !matches!(r.tone, Tone::Solid), path: r.path, rule: r.rule, ink, overprint: r.overprint })
        .collect();
    let mut per_ink = vec![0usize; inks.len()];
    for s in &shapes {
        if let Some(n) = per_ink.get_mut(s.ink) {
            *n += 1;
        }
    }

    let (plates, screened_parts) = compose_parts(&shapes, inks.len());
    if screened_parts.len() > MAX_SCREENED {
        return Err(bad(
            C,
            format!(
                "{} tinted or gradient shapes show; at most {MAX_SCREENED} can be screened at once: use `tones: false` or simplify",
                screened_parts.len()
            ),
        ));
    }
    let tone_screen = if screened_parts.is_empty() {
        None
    } else {
        let curve = curve_of(p, C)?;
        let mut scatter = scatter_of(C, p, dpi)?;
        if curve.is_some()
            && let Some(sc) = scatter.as_mut()
        {
            sc.fm.dot_gain = 0.0;
        }
        Some(ToneScreen {
            opts: HalftoneOptions {
                dpi,
                lpi: num(p, C, "lpi", 45.0, 1.0..=500.0)?,
                angle_deg: 0.0,
                shape: shape_of(p, C)?,
                min_dot: num(p, C, "minDot", 0.08, 0.0..=1.0)?,
                max_dot: num(p, C, "maxDot", 0.95, 0.0..=1.0)?,
                dot_gain: if curve.is_some() { 0.0 } else { num(p, C, "dotGain", 0.0, 0.0..=MAX_DOT_GAIN)? },
            },
            scatter,
            curve,
        })
    };
    // Print order: light to dark, as for the other methods.
    let lightness = |i: usize| inks.get(i).map_or(0.0, |k| srgb_to_lab(k.rgb)[0]);
    let mut order: Vec<usize> = (0..inks.len()).collect();
    order.sort_by(|&a, &b| lightness(b).total_cmp(&lightness(a)));
    let ordered: Vec<PathData> = order.iter().filter_map(|&i| plates.get(i).cloned()).collect();
    let pairs = pair_traps(p, &inks)?;
    let trapped = if pairs.is_empty() {
        trap_under(&ordered, pt(trap_mm))
    } else {
        // `ordered` holds the inks in print order: position -> ink index in `order`.
        trap_pairs(&ordered, |k, j| {
            let (a, b) = (order.get(k).copied(), order.get(j).copied());
            let own = pairs.iter().rev().find(|(sp, un, _)| Some(*sp) == a && Some(*un) == b);
            own.map_or(trap_mm, |(_, _, mm)| *mm) / 25.4 * 72.0
        })
    };

    let mut renderer = Renderer::new();
    let mut films = Vec::with_capacity(inks.len() + 1);
    // The path behind each film, in the same order, for the vector output.
    let mut film_paths: Vec<PathData> = Vec::with_capacity(inks.len() + 1);
    if let Some(ub) = &underbase {
        let mut covered: Vec<(&PathData, FillRule)> =
            order.iter().filter(|&&i| lightness(i) < WHITE_L).filter_map(|&i| plates.get(i)).map(|q| (q, FillRule::NonZero)).collect();
        // Dots print on the underbase too.
        covered.extend(
            screened_parts
                .iter()
                .filter(|(idx, _)| shapes.get(*idx).is_some_and(|sh| lightness(sh.ink) < WHITE_L))
                .map(|(_, q)| (q, FillRule::NonZero)),
        );
        let footprint = unite_all(&covered);
        if !footprint.is_empty() {
            let path = offset_path(&footprint, -ub.choke_px, Join::Round, MITER_LIMIT);
            films.push(Film {
                name: ub.ink.name.clone(),
                ink_rgb: ub.ink.rgb,
                mesh: ub.ink.mesh,
                role: Role::Underbase,
                bitmap: rasterize(&mut renderer, &path, region, scale, size)?,
            });
            film_paths.push(path);
        }
    }
    for (&i, plate) in order.iter().zip(&trapped) {
        let Some(ink) = inks.get(i) else { continue };
        let mut bitmap = rasterize(&mut renderer, plate, region, scale, size)?;
        if let Some(screen) = &tone_screen {
            let angle = DEFAULT_ANGLES.get(i % DEFAULT_ANGLES.len()).copied().unwrap_or(22.5);
            for (idx, part) in screened_parts.iter().filter(|(idx, _)| shapes.get(*idx).is_some_and(|sh| sh.ink == i)) {
                let (Some(shape), Some(tone)) = (shapes.get(*idx), tones.get(*idx)) else { continue };
                let dots = screened_part(&mut renderer, shape, tone, part, ink.rgb, angle, screen, (region, scale, size))?;
                for y in 0..bitmap.height() {
                    for x in 0..bitmap.width() {
                        if dots.get(x, y) {
                            bitmap.set(x as i64, y as i64, true);
                        }
                    }
                }
            }
        }
        films.push(Film { name: ink.name.clone(), ink_rgb: ink.rgb, mesh: ink.mesh, role: Role::Color, bitmap });
        film_paths.push(plate.clone());
    }

    // The plates as paths in the document's own space, kept for `intoLayers`.
    let layer_plates: Vec<(String, [u8; 3], PathData)> = if bool_or(p, "intoLayers", false) {
        films.iter().zip(&film_paths).map(|(f, path)| (f.name.clone(), f.ink_rgb, path.clone())).collect()
    } else {
        Vec::new()
    };
    let (mut films, dcs) = finish_films(C, p, films, dpi, marks, fmt)?;
    if let Some(format) = vector_format {
        let framed = if marks { Some(vector_marks(region, dpi)?) } else { None };
        // Mirrored films come with mirrored vector plates: flip about the middle of the page they are drawn on.
        if bool_or(p, "mirror", false) {
            let page = framed.as_ref().map_or(region, |(_, r)| *r);
            let flip = vectorcraft_geom::Affine::new([-1.0, 0.0, 0.0, 1.0, page.x0 + page.x1, 0.0]);
            film_paths = film_paths.iter().map(|path| path.transformed(flip)).collect();
        }
        attach_vector_pdfs(&mut films, &film_paths, region, framed.as_ref(), dir_param(p), format)?;
    }
    let (w, h) = films.first().map(|f| (f["width"].clone(), f["height"].clone())).unwrap_or_default();
    let sheet = job_sheet(C, p, dpi, &films, &[], dir_param(p))?;

    let mut notes: Vec<String> = Vec::new();
    if collected.transparent > 0 {
        notes.push(format!("{} objects use opacity, blend modes, masks or effects; they print as solid shapes", collected.transparent));
    }
    if collected.tinted > 0 {
        notes.push(format!("{} spot tints below 100% print solid", collected.tinted));
    }
    if !screened_parts.is_empty() {
        notes.push(format!(
            "{} tinted, see-through or gradient shapes print as dots in the films; the vector plates (vectorPdf, vectorSvg) hold only the solid parts",
            screened_parts.len()
        ));
    }
    if collected.mixed_text > 0 {
        notes.push(format!("{} text objects print in the colours of their first style: Create Outlines to separate the rest", collected.mixed_text));
    }
    let skipped: Vec<Value> = collected.skipped.iter().map(|(object, why)| json!({ "object": object, "why": why })).collect();
    let ink_list: Vec<Value> =
        inks.iter().zip(&per_ink).map(|(k, n)| json!({ "name": k.name, "color": sc_core::color::to_hex(k.rgb), "shapes": n })).collect();
    let mut out = json!({
        "dpi": dpi, "width": w, "height": h, "films": films, "inks": ink_list,
        "skipped": skipped, "skippedTotal": collected.skipped_total, "notes": notes,
    });
    if !layer_plates.is_empty() {
        out["layers"] = json!(plates_into_layers(s, layer_plates)?);
    }
    attach(&mut out, sheet);
    attach_dcs(&mut out, dcs);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use vectorcraft_geom::{Anchor, Point, SubPath};

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> PathData {
        let c = |x, y| Anchor::corner(Point::new(x, y));
        PathData::single(SubPath::new(vec![c(x0, y0), c(x1, y0), c(x1, y1), c(x0, y1)], true))
    }

    fn shape(path: PathData, ink: usize, overprint: bool) -> Shape {
        Shape { path, rule: FillRule::NonZero, ink, overprint, screened: false }
    }

    fn area_of(p: &PathData) -> f64 {
        vectorcraft_pathops::area(p, FillRule::NonZero)
    }

    #[test]
    fn a_shape_on_top_knocks_out_the_ink_below_it() {
        // Ink 0: a 100 × 100 square; ink 1: a 40 × 40 square on top of it.
        let shapes = [shape(rect(0.0, 0.0, 100.0, 100.0), 0, false), shape(rect(30.0, 30.0, 70.0, 70.0), 1, false)];
        let plates = compose(&shapes, 2);
        assert!((area_of(&plates[0]) - (10_000.0 - 1_600.0)).abs() < 1e-3, "{}", area_of(&plates[0]));
        assert!((area_of(&plates[1]) - 1_600.0).abs() < 1e-3);
    }

    #[test]
    fn an_overprinting_shape_leaves_the_ink_below_whole() {
        let shapes = [shape(rect(0.0, 0.0, 100.0, 100.0), 0, false), shape(rect(30.0, 30.0, 70.0, 70.0), 1, true)];
        let plates = compose(&shapes, 2);
        assert!((area_of(&plates[0]) - 10_000.0).abs() < 1e-3);
        assert!((area_of(&plates[1]) - 1_600.0).abs() < 1e-3);
    }

    #[test]
    fn only_shapes_above_knock_out_and_the_same_ink_never_does() {
        // The big shape is above the small one: the small one is hidden. Same-ink shapes just join.
        let shapes = [
            shape(rect(30.0, 30.0, 70.0, 70.0), 1, false),
            shape(rect(0.0, 0.0, 100.0, 100.0), 0, false),
            shape(rect(90.0, 0.0, 140.0, 40.0), 0, false),
        ];
        let plates = compose(&shapes, 2);
        assert!(plates[1].is_empty(), "covered");
        assert!((area_of(&plates[0]) - (10_000.0 + 40.0 * 40.0)).abs() < 1e-3, "{}", area_of(&plates[0]));
    }

    #[test]
    fn a_hole_stays_a_hole() {
        // A ring of ink 0 (square with a square hole, even-odd) with ink 1 inside the hole.
        let mut ring = rect(0.0, 0.0, 100.0, 100.0);
        ring.subpaths.extend(rect(30.0, 30.0, 70.0, 70.0).subpaths);
        let shapes =
            [Shape { path: ring, rule: FillRule::EvenOdd, ink: 0, overprint: false, screened: false }, shape(rect(40.0, 40.0, 60.0, 60.0), 1, false)];
        let plates = compose(&shapes, 2);
        assert!((area_of(&plates[0]) - (10_000.0 - 1_600.0)).abs() < 1e-3);
        assert!((area_of(&plates[1]) - 400.0).abs() < 1e-3);
    }

    #[test]
    fn a_trap_grows_the_light_plate_only_under_the_dark_one() {
        // Light on the left (0..50), dark on the right (50..100): trap 2.
        let plates = [rect(0.0, 0.0, 50.0, 100.0), rect(50.0, 0.0, 100.0, 100.0)];
        let t = trap_under(&plates, 2.0);
        // The light plate gains a 2 × 100 strip under the dark one and nothing elsewhere.
        assert!((area_of(&t[0]) - (5_000.0 + 200.0)).abs() < 1.0, "{}", area_of(&t[0]));
        let b = t[0].bounds().unwrap();
        assert!((b.x0 - 0.0).abs() < 1e-6 && (b.y0 - 0.0).abs() < 1e-6 && (b.x1 - 52.0).abs() < 0.01 && (b.y1 - 100.0).abs() < 1e-6, "{b:?}");
        assert_eq!(area_of(&t[1]), area_of(&plates[1]), "the last plate is not spread");
        // No trap: unchanged. A lone plate has nothing to tuck under.
        assert_eq!(trap_under(&plates, 0.0), plates.to_vec());
        assert_eq!(trap_under(&plates[..1], 2.0), plates[..1].to_vec());
    }

    #[test]
    fn each_pair_can_have_its_own_trap() {
        // Light, mid and dark side by side: the light one touches the mid, the mid touches the dark.
        let plates = [rect(0.0, 0.0, 50.0, 100.0), rect(50.0, 0.0, 100.0, 100.0), rect(100.0, 0.0, 150.0, 100.0)];
        let t = trap_pairs(&plates, |k, j| if (k, j) == (0, 1) { 4.0 } else { 1.0 });
        assert!((area_of(&t[0]) - (5_000.0 + 400.0)).abs() < 1.0, "{}", area_of(&t[0]));
        assert!((area_of(&t[1]) - (5_000.0 + 100.0)).abs() < 1.0, "{}", area_of(&t[1]));
        assert_eq!(area_of(&t[2]), 5_000.0);
        // Zero for a pair leaves it untrapped.
        let t = trap_pairs(&plates, |k, j| if (k, j) == (0, 1) { 0.0 } else { 1.0 });
        assert_eq!(area_of(&t[0]), 5_000.0);
        // The same width everywhere agrees with the single-width trap.
        let (a, b) = (trap_pairs(&plates, |_, _| 2.0), trap_under(&plates, 2.0));
        for (x, y) in a.iter().zip(&b) {
            assert!((area_of(x) - area_of(y)).abs() < 1.0);
        }
    }

    #[test]
    fn vector_marks_sit_outside_the_art_inside_the_framed_region_and_match_the_raster_margin() {
        let art = Rect::new(0.0, 0.0, 200.0, 100.0);
        let (marks, framed) = vector_marks(art, 100.0).unwrap();
        // The raster film grows by (gap + size + 1) px = 64 px a side at 100 dpi, i.e. 46.08 pt.
        assert!((framed.x0 + 46.08).abs() < 1e-9 && (framed.x1 - 246.08).abs() < 1e-9 && (framed.y1 - 146.08).abs() < 1e-9, "{framed:?}");
        let b = marks.bounds().unwrap();
        assert!(b.x0 >= framed.x0 && b.y0 >= framed.y0 && b.x1 <= framed.x1 && b.y1 <= framed.y1, "{b:?}");
        let inside = boolean(&marks, FillRule::NonZero, &rect(0.0, 0.0, 200.0, 100.0), FillRule::NonZero, BoolOp::Intersect);
        assert!(inside.is_empty(), "nothing on the art");
        // Four marks of the same shape: a ring (outer r 13.68, line 0.72) plus a cross 27.36 long.
        let one = area_of(&marks) / 4.0;
        let (r, t) = (13.68_f64, 0.72_f64);
        let ring = std::f64::consts::PI * (r * r - (r - t) * (r - t));
        let cross = 2.0 * 2.0 * r * t - t * t;
        // The cross overlaps the ring where they meet, so the mark is a little less than the sum.
        assert!(one > 0.9 * (ring + cross) && one < ring + cross + 1.0, "{one} vs {}", ring + cross);
        assert!(vector_marks(art, f64::NAN).is_err());
    }

    #[test]
    fn plates_become_hard_edged_film() {
        let mut r = Renderer::new();
        let region = Rect::new(0.0, 0.0, 100.0, 50.0);
        // 100 pt at 2 px a point: the square 10..60 × 10..40 covers pixels 20..120 × 20..80.
        let film = rasterize(&mut r, &rect(10.0, 10.0, 60.0, 40.0), region, 2.0, (200, 100)).unwrap();
        assert!(film.get(20, 20) && film.get(119, 79) && film.get(70, 50));
        assert!(!film.get(19, 50) && !film.get(120, 50) && !film.get(70, 19) && !film.get(70, 80));
        assert!((film.coverage() - 100.0 * 60.0 / 20_000.0).abs() < 0.01, "{}", film.coverage());
    }
}

//! Renders the Rusty* suite logos and the standalone R mark to PNG.
//!
//! - 5 app-icon PNGs: rounded gradient tile + white custom R (logos/<app>.png)
//! - standalone R mark, transparent background, white and black (mark/R-mark-*.png)
//!
//! The R outline is the real Avenir Next Demi Bold "R" with the bowl/leg bridge to
//! the stem removed (clean vertical cut, open notch); see src/extract-glyph for how
//! the base outline was pulled from the font. Gradients reproduce the board's
//! CSS `linear-gradient(130deg, A 0%, B 75%)`.

use resvg::tiny_skia;
use resvg::usvg;

/// Custom R outline (font units, y-down). Ink bbox: x 0..507, y 0..708.
const R_PATH: &str = "M0 0 L246 0 Q296 0 343 10.5 Q390 21 426.5 45 Q463 69 485 108 Q507 147 507 203 Q507 280 463.5 328.5 Q420 377 345 394 L507 708 L382 708 L218 394 L218 305 Q256 305 283 301 Q310 297 331.5 286 Q353 275 367 254.5 Q381 234 381 202 Q381 173 368 154.5 Q355 136 335 125 Q315 114 289.5 110 Q264 106 240 106 L126 106 L126 708 L0 708 Z";
const RW: f64 = 507.0;
const RH: f64 = 708.0;

/// Gradient endpoint vector matching CSS `linear-gradient(<deg>)` over a square.
fn grad_vec(size: f64, deg: f64) -> (f64, f64, f64, f64) {
    let a = deg.to_radians();
    let (dx, dy) = (a.sin(), -a.cos()); // CSS angle -> (x right, y down)
    let l = size * (a.sin().abs() + a.cos().abs());
    let (cx, cy) = (size / 2.0, size / 2.0);
    (cx - dx * l / 2.0, cy - dy * l / 2.0, cx + dx * l / 2.0, cy + dy * l / 2.0)
}

/// Translate/scale to geometrically center the R at `height_ratio` of the canvas.
fn r_transform(size: f64, height_ratio: f64) -> (f64, f64, f64) {
    let rh = height_ratio * size;
    let scale = rh / RH;
    let rw = RW * scale;
    ((size - rw) / 2.0, (size - rh) / 2.0, scale)
}

/// Inner icon content (gradient + rounded tile + R), no outer <svg>. `gid` must be unique.
fn icon_inner(a: &str, b: &str, size: f64, gid: &str) -> String {
    let (x1, y1, x2, y2) = grad_vec(size, 130.0);
    let (tx, ty, sc) = r_transform(size, 0.63);
    let rx = 0.2246 * size; // iOS-style corner radius
    format!(
        "<defs><linearGradient id=\"{gid}\" gradientUnits=\"userSpaceOnUse\" x1=\"{x1:.2}\" y1=\"{y1:.2}\" x2=\"{x2:.2}\" y2=\"{y2:.2}\">\
<stop offset=\"0\" stop-color=\"{a}\"/><stop offset=\"0.75\" stop-color=\"{b}\"/></linearGradient></defs>\
<rect x=\"0\" y=\"0\" width=\"{size}\" height=\"{size}\" rx=\"{rx:.1}\" fill=\"url(#{gid})\"/>\
<g transform=\"translate({tx:.2} {ty:.2}) scale({sc:.5})\"><path fill=\"#ffffff\" d=\"{path}\"/></g>",
        path = R_PATH
    )
}

fn icon_svg(a: &str, b: &str, size: f64) -> String {
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{size}\" height=\"{size}\" viewBox=\"0 0 {size} {size}\">{}</svg>",
        icon_inner(a, b, size, "g")
    )
}

/// Wide contact sheet: all logos in a row on the dark Rustyfin background.
fn contact_sheet_svg(logos: &[(&str, &str, &str)], tile: f64, gap: f64, pad: f64) -> String {
    let n = logos.len() as f64;
    let w = n * tile + (n - 1.0) * gap + 2.0 * pad;
    let h = tile + 2.0 * pad;
    let mut s = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\">\
<rect x=\"0\" y=\"0\" width=\"{w}\" height=\"{h}\" fill=\"#1d2231\"/>"
    );
    for (i, (name, a, b)) in logos.iter().enumerate() {
        let x = pad + i as f64 * (tile + gap);
        s.push_str(&format!(
            "<svg x=\"{x:.1}\" y=\"{pad:.1}\" width=\"{tile}\" height=\"{tile}\" viewBox=\"0 0 {tile} {tile}\">{}</svg>",
            icon_inner(a, b, tile, &format!("g{name}"))
        ));
    }
    s.push_str("</svg>");
    s
}

fn mark_svg(fill: &str, size: f64) -> String {
    let (tx, ty, sc) = r_transform(size, 0.80);
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{size}\" height=\"{size}\" viewBox=\"0 0 {size} {size}\">\
<g transform=\"translate({tx:.2} {ty:.2}) scale({sc:.5})\"><path fill=\"{fill}\" d=\"{path}\"/></g></svg>",
        path = R_PATH
    )
}

fn render_wh(svg: &str, w: u32, h: u32, out: &str) {
    let tree = usvg::Tree::from_str(svg, &usvg::Options::default()).expect("parse svg");
    let mut pm = tiny_skia::Pixmap::new(w, h).expect("pixmap");
    resvg::render(&tree, tiny_skia::Transform::identity(), &mut pm.as_mut());
    pm.save_png(out).expect("save png");
    println!("wrote {out}");
}

fn render(svg: &str, size: u32, out: &str) {
    render_wh(svg, size, size, out);
}

fn main() {
    let base = std::env::args().nth(1).unwrap_or_else(|| ".".to_string());
    std::fs::create_dir_all(format!("{base}/logos")).unwrap();
    std::fs::create_dir_all(format!("{base}/mark")).unwrap();
    let size = 1024.0;

    // (app, colorA, colorB) — final board colorways (reversals already applied)
    let logos = [
        ("rustyfin", "#ff914d", "#b18cff"),     // ember:    orange -> purple
        ("rustynet", "#dd7bd6", "#ff8f67"),      // flame rev: magenta -> orange
        ("rustychat", "#b68afc", "#64baff"),     // twilight rev: violet -> blue
        ("rustydns", "#94aeff", "#01bcc3"),      // nebula:   periwinkle -> teal
        ("rustytorrent", "#01c5ee", "#01c381"),  // lagoon:   cyan -> emerald
    ];
    for (name, a, b) in logos {
        render(&icon_svg(a, b, size), size as u32, &format!("{base}/logos/{name}.png"));
    }
    render(&mark_svg("#ffffff", size), size as u32, &format!("{base}/mark/R-mark-white.png"));
    render(&mark_svg("#000000", size), size as u32, &format!("{base}/mark/R-mark-black.png"));

    // contact sheet: all logos in a row (README hero / quick reference)
    std::fs::create_dir_all(format!("{base}/preview")).unwrap();
    let (tile, gap, pad) = (360.0, 36.0, 48.0);
    let n = logos.len() as f64;
    let (cw, ch) = ((n * tile + (n - 1.0) * gap + 2.0 * pad) as u32, (tile + 2.0 * pad) as u32);
    render_wh(&contact_sheet_svg(&logos, tile, gap, pad), cw, ch, &format!("{base}/preview/contact-sheet.png"));
}

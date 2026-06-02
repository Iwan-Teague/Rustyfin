// Compute a warm OKLCH panorama ramp for the Rusty* suite and emit a preview HTML.
// Pure std, no external crates. Bjorn Ottosson OKLab conversions.

fn srgb_to_lin(c: f64) -> f64 {
    let c = c / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}
fn lin_to_srgb(c: f64) -> f64 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

fn rgb_to_oklch(r: f64, g: f64, b: f64) -> (f64, f64, f64) {
    let (r, g, b) = (srgb_to_lin(r), srgb_to_lin(g), srgb_to_lin(b));
    let l = 0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b;
    let m = 0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b;
    let s = 0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b;
    let (l_, m_, s_) = (l.cbrt(), m.cbrt(), s.cbrt());
    let big_l = 0.2104542553 * l_ + 0.7936177850 * m_ - 0.0040720468 * s_;
    let a = 1.9779984951 * l_ - 2.4285922050 * m_ + 0.4505937099 * s_;
    let bb = 0.0259040371 * l_ + 0.7827717662 * m_ - 0.8086757660 * s_;
    let c = a.hypot(bb);
    let h = bb.atan2(a).to_degrees().rem_euclid(360.0);
    (big_l, c, h)
}

fn oklch_to_lin(big_l: f64, c: f64, h: f64) -> (f64, f64, f64) {
    let a = c * h.to_radians().cos();
    let b = c * h.to_radians().sin();
    let l_ = big_l + 0.3963377774 * a + 0.2158037573 * b;
    let m_ = big_l - 0.1055613458 * a - 0.0638541728 * b;
    let s_ = big_l - 0.0894841775 * a - 1.2914855480 * b;
    let (l, m, s) = (l_.powi(3), m_.powi(3), s_.powi(3));
    let r = 4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s;
    let g = -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s;
    let b2 = -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s;
    (r, g, b2)
}

fn in_gamut(big_l: f64, c: f64, h: f64) -> bool {
    let (r, g, b) = oklch_to_lin(big_l, c, h);
    let eps = 1e-4;
    [r, g, b].iter().all(|&v| v >= -eps && v <= 1.0 + eps)
}

fn max_chroma(big_l: f64, h: f64) -> f64 {
    let (mut lo, mut hi) = (0.0_f64, 0.4_f64);
    for _ in 0..40 {
        let mid = (lo + hi) / 2.0;
        if in_gamut(big_l, mid, h) { lo = mid } else { hi = mid }
    }
    lo
}

fn oklch_to_hex(big_l: f64, c: f64, h: f64) -> String {
    let (r, g, b) = oklch_to_lin(big_l, c, h);
    let q = |v: f64| (lin_to_srgb(v) * 255.0).round().clamp(0.0, 255.0) as u8;
    format!("#{:02x}{:02x}{:02x}", q(r), q(g), q(b))
}

fn main() {
    // ---- current brand anchors ----
    let anchors = [
        ("orange (#ff914d)", (255.0, 145.0, 77.0)),
        ("pink (#ff7588)", (255.0, 117.0, 136.0)),
        ("purple (#b18cff)", (177.0, 140.0, 255.0)),
    ];
    println!("=== current brand anchors in OKLCH ===");
    let mut ok = Vec::new();
    for (name, (r, g, b)) in anchors {
        let (l, c, h) = rgb_to_oklch(r, g, b);
        println!("  {:18} L={:.3} C={:.3} H={:6.1}", name, l, c, h);
        ok.push((l, c, h));
    }
    let (lo, _, ho) = ok[0]; // orange
    let (lp, _, hp) = ok[2]; // purple

    // warm arc: orange hue -> purple hue the WARM way (decreasing, wrap thru 360)
    let h_start = ho;
    let h_end = hp - 360.0; // negative so sweep passes thru red/magenta
    println!(
        "\nwarm arc: {:.1}deg -> {:.1}deg (thru red/magenta), span {:.1}deg",
        h_start,
        h_end + 360.0,
        h_start - h_end
    );

    let n_tiles = 8;
    let l_ramp = ((lo + lp) / 2.0 * 1000.0).round() / 1000.0;
    let bound_h: Vec<f64> = (0..=n_tiles)
        .map(|i| h_start + (h_end - h_start) * i as f64 / n_tiles as f64)
        .collect();
    let min_max_c = bound_h
        .iter()
        .map(|&h| max_chroma(l_ramp, h.rem_euclid(360.0)))
        .fold(f64::INFINITY, f64::min);
    let c_ramp = (0.96 * min_max_c * 1000.0).round() / 1000.0;
    println!("shared L={}  shared C={}", l_ramp, c_ramp);

    let bh: Vec<f64> = bound_h.iter().map(|h| h.rem_euclid(360.0)).collect();
    let bhex: Vec<String> = bh.iter().map(|&h| oklch_to_hex(l_ramp, c_ramp, h)).collect();

    let apps = [
        "rustyfin", "rustychat", "rustynet", "rustytorrent", "rustydns",
        "rusty\u{b7} (spare)", "rusty\u{b7} (spare)", "rusty\u{b7} (spare)",
    ];

    println!("\n=== per-app tiles (2-stop, 130deg, 0%->75%) ===");
    // (name, h1, h2, hex1, hex2)
    let mut tiles: Vec<(&str, f64, f64, String, String)> = Vec::new();
    for (i, name) in apps.iter().enumerate() {
        let (h1, h2) = (bh[i], bh[i + 1]);
        println!("  {:18} H {:6.1}->{:6.1}   {} -> {}", name, h1, h2, bhex[i], bhex[i + 1]);
        tiles.push((name, h1, h2, bhex[i].clone(), bhex[i + 1].clone()));
    }

    // ---- emit HTML ----
    let current = "linear-gradient(130deg, #ff914d 0%, #b18cff 75%)";
    let master_stops: String = bhex
        .iter()
        .enumerate()
        .map(|(i, hx)| format!("{} {}%", hx, (i as f64 / n_tiles as f64 * 100.0).round()))
        .collect::<Vec<_>>()
        .join(", ");

    let dock: String = tiles
        .iter()
        .map(|t| {
            format!(
                "<span class=\"dockR\" style=\"background:linear-gradient(130deg,{} 0%,{} 75%)\">R</span>",
                t.3, t.4
            )
        })
        .collect();

    let cards: String = tiles
        .iter()
        .map(|t| {
            format!(
                "<div class=\"card\">\
                   <div class=\"glyph\" style=\"background:linear-gradient(130deg,{h1hex} 0%,{h2hex} 75%)\">R</div>\
                   <div class=\"name\">{name}</div>\
                   <div class=\"meta\">H {h1:.0}\u{b0} \u{2192} {h2:.0}\u{b0}</div>\
                   <div class=\"sw\"><span style=\"background:{h1hex}\"></span><span style=\"background:{h2hex}\"></span></div>\
                   <code>--logo-h: {h1:.1};</code>\
                   <code class=\"dim\">{h1hex} \u{2192} {h2hex}</code>\
                 </div>",
                name = t.0, h1 = t.1, h2 = t.2, h1hex = t.3, h2hex = t.4
            )
        })
        .collect();

    let mut html = String::new();
    html.push_str(r#"<!doctype html><html><head><meta charset="utf-8"><title>Rusty* suite palette</title>
<style>
  :root { --bg-start:#1d2231; --bg-end:#242b3c; }
  * { box-sizing:border-box; }
  body { margin:0; font:15px/1.5 -apple-system,Segoe UI,Roboto,sans-serif; color:#f8f8ff;
    background:linear-gradient(150deg,var(--bg-start) 0%,var(--bg-end) 55%,#2b3347 100%); min-height:100vh; padding:48px; }
  h1 { font-size:22px; font-weight:650; margin:0 0 4px; }
  h2 { font-size:13px; font-weight:600; letter-spacing:.08em; text-transform:uppercase; color:#c4c9e1; margin:44px 0 16px; }
  p.sub { color:#c4c9e1; margin:0 0 8px; max-width:780px; }
  .clip { -webkit-background-clip:text; background-clip:text; color:transparent; }
  .dock { display:inline-flex; gap:18px; align-items:center; padding:18px 26px; border-radius:26px;
    background:linear-gradient(180deg,rgba(35,41,59,.96),rgba(27,33,50,.96)); border:1px solid rgba(215,223,255,.14);
    box-shadow:0 20px 50px rgba(0,0,0,.35); }
  .dockR { font-size:46px; font-weight:680; line-height:1; -webkit-background-clip:text; background-clip:text; color:transparent; }
  .ramp { height:26px; border-radius:13px; border:1px solid rgba(215,223,255,.14); background:linear-gradient(90deg, "#);
    html.push_str(&master_stops);
    html.push_str(r#"); }
  .ticks { display:flex; justify-content:space-between; font-size:11px; color:#9aa0bf; margin-top:6px; }
  .grid { display:grid; grid-template-columns:repeat(4,1fr); gap:16px; max-width:920px; }
  .card { background:rgba(39,45,64,.55); border:1px solid rgba(215,223,255,.12); border-radius:16px; padding:18px; }
  .glyph { font-size:52px; font-weight:680; line-height:1; -webkit-background-clip:text; background-clip:text; color:transparent; }
  .name { font-weight:600; margin-top:10px; }
  .meta { font-size:12px; color:#9aa0bf; }
  .sw { display:flex; gap:6px; margin:10px 0 8px; }
  .sw span { width:34px; height:14px; border-radius:4px; }
  code { display:block; font:12px/1.6 ui-monospace,Menlo,monospace; color:#ffc27a; }
  code.dim { color:#9aa0bf; }
  .twoUp { display:flex; gap:40px; align-items:flex-end; flex-wrap:wrap; }
  .cmp { text-align:center; }
  .cmp .g { font-size:60px; font-weight:680; -webkit-background-clip:text; background-clip:text; color:transparent; }
  .cmp small { color:#9aa0bf; }
</style></head><body>
"#);
    html.push_str(&format!(
        "<h1><span class=\"clip\" style=\"background:{current}\">Rusty*</span> suite \u{2014} warm panorama palette</h1>\
         <p class=\"sub\">One shared OKLCH ramp (L={l}, C={c}, hue {h0:.0}\u{b0}\u{2192}{hn:.0}\u{b0} through red/magenta), sliced into 8 tiles. Constant L &amp; C \u{2192} even brightness, no icon out-shouts its neighbors.</p>",
        current = current, l = l_ramp, c = c_ramp, h0 = bh[0], hn = bh[bh.len() - 1]
    ));
    html.push_str("<h2>The tray test \u{2014} 8 apps docked side by side</h2><div class=\"dock\">");
    html.push_str(&dock);
    html.push_str("</div>");
    html.push_str("<h2>Master ramp (the seamless gradient the tiles cut from)</h2><div class=\"ramp\"></div>");
    html.push_str(&format!(
        "<div class=\"ticks\"><span>{}</span><span>orange \u{2192} red \u{2192} magenta \u{2192} violet</span><span>{}</span></div>",
        bhex[0], bhex[bhex.len() - 1]
    ));
    html.push_str("<h2>Per-app tiles \u{2014} one hue knob each</h2><div class=\"grid\">");
    html.push_str(&cards);
    html.push_str("</div>");
    html.push_str(&format!(
        "<h2>Old vs new flagship tile</h2><div class=\"twoUp\">\
           <div class=\"cmp\"><div class=\"g\" style=\"background:{current}\">R</div><small>current rustyfin<br>orange\u{2192}purple (full sweep)</small></div>\
           <div class=\"cmp\"><div class=\"g\" style=\"background:linear-gradient(130deg,{h1} 0%,{h2} 75%)\">R</div><small>new rustyfin tile<br>{h1} \u{2192} {h2}</small></div>\
         </div>",
        current = current, h1 = tiles[0].3, h2 = tiles[0].4
    ));
    html.push_str("</body></html>");

    std::fs::write("/tmp/rustyfin-suite-palette.html", html).unwrap();
    println!("\nwrote /tmp/rustyfin-suite-palette.html");
}

// Rusty* suite: keep rustyfin's 2-stop recipe (its two L/C points + hue interval),
// rotate the whole pair around the wheel per app. Locked L/C => cohesive tray.
// Pure std, Bjorn Ottosson OKLab.

fn srgb_to_lin(c: f64) -> f64 { let c = c/255.0; if c<=0.04045 {c/12.92} else {((c+0.055)/1.055).powf(2.4)} }
fn lin_to_srgb(c: f64) -> f64 { let c = c.clamp(0.0,1.0); if c<=0.0031308 {c*12.92} else {1.055*c.powf(1.0/2.4)-0.055} }

fn rgb_to_oklch(r: f64, g: f64, b: f64) -> (f64,f64,f64) {
    let (r,g,b)=(srgb_to_lin(r),srgb_to_lin(g),srgb_to_lin(b));
    let l=0.4122214708*r+0.5363325363*g+0.0514459929*b;
    let m=0.2119034982*r+0.6806995451*g+0.1073969566*b;
    let s=0.0883024619*r+0.2817188376*g+0.6299787005*b;
    let (l_,m_,s_)=(l.cbrt(),m.cbrt(),s.cbrt());
    let big_l=0.2104542553*l_+0.7936177850*m_-0.0040720468*s_;
    let a=1.9779984951*l_-2.4285922050*m_+0.4505937099*s_;
    let bb=0.0259040371*l_+0.7827717662*m_-0.8086757660*s_;
    (big_l, a.hypot(bb), bb.atan2(a).to_degrees().rem_euclid(360.0))
}
fn oklch_to_lin(big_l: f64, c: f64, h: f64) -> (f64,f64,f64) {
    let a=c*h.to_radians().cos(); let b=c*h.to_radians().sin();
    let l_=big_l+0.3963377774*a+0.2158037573*b;
    let m_=big_l-0.1055613458*a-0.0638541728*b;
    let s_=big_l-0.0894841775*a-1.2914855480*b;
    let (l,m,s)=(l_.powi(3),m_.powi(3),s_.powi(3));
    (4.0767416621*l-3.3077115913*m+0.2309699292*s,
     -1.2684380046*l+2.6097574011*m-0.3413193965*s,
     -0.0041960863*l-0.7034186147*m+1.7076147010*s)
}
fn in_gamut(l: f64, c: f64, h: f64) -> bool {
    let (r,g,b)=oklch_to_lin(l,c,h); let e=1e-4;
    r>=-e&&r<=1.0+e&&g>=-e&&g<=1.0+e&&b>=-e&&b<=1.0+e
}
fn max_chroma(l: f64, h: f64) -> f64 {
    let (mut lo,mut hi)=(0.0_f64,0.4_f64);
    for _ in 0..40 { let mid=(lo+hi)/2.0; if in_gamut(l,mid,h){lo=mid}else{hi=mid} } lo
}
fn hex(l: f64, c: f64, h: f64) -> String {
    // clamp chroma into gamut, keep L and H
    let c = if in_gamut(l,c,h) { c } else { max_chroma(l,h)*0.999 };
    let (r,g,b)=oklch_to_lin(l,c,h);
    let q=|v: f64| (lin_to_srgb(v)*255.0).round().clamp(0.0,255.0) as u8;
    format!("#{:02x}{:02x}{:02x}", q(r),q(g),q(b))
}
fn hue_name(h: f64) -> &'static str {
    match h.rem_euclid(360.0) as i32 {
        15..=39 => "red", 40..=74 => "orange", 75..=114 => "yellow", 115..=164 => "green",
        165..=214 => "teal", 215..=264 => "blue", 265..=294 => "indigo", 295..=324 => "violet",
        325..=344 => "magenta", _ => "pink",
    }
}

fn main() {
    // rustyfin's two stops -> reuse their exact L,C; rotate H by per-app r.
    let (la,ca,ha)=rgb_to_oklch(255.0,145.0,77.0);  // orange
    let (lb,cb,hb)=rgb_to_oklch(177.0,140.0,255.0); // purple
    println!("rustyfin recipe: A L={:.3} C={:.3} H={:.1} | B L={:.3} C={:.3} H={:.1} | interval {:.0}deg",
             la,ca,ha,lb,cb,hb,(hb-ha).rem_euclid(360.0));

    let apps: [(&str,f64);8] = [
        ("rustyfin",0.0),("rustychat",45.0),("rustynet",90.0),("rustytorrent",135.0),
        ("rustydns",180.0),("rusty\u{b7} spare",225.0),("rusty\u{b7} spare",270.0),("rusty\u{b7} spare",315.0),
    ];

    // (name, hexA, hexB, leadName, rot, hA, hB)
    let mut tiles: Vec<(String,String,String,String,f64,f64,f64)> = Vec::new();
    println!("\n=== per-app pairs (rustyfin recipe rotated; locked L/C) ===");
    for (name,r) in apps {
        let (ax,bx,hA,hB) = if r==0.0 {
            // keep rustyfin EXACT
            ("#ff914d".to_string(),"#b18cff".to_string(),ha,hb)
        } else {
            let hA=(ha+r).rem_euclid(360.0); let hB=(hb+r).rem_euclid(360.0);
            (hex(la,ca,hA),hex(lb,cb,hB),hA,hB)
        };
        let lead=hue_name(hA);
        println!("  {:16} +{:3.0}deg  {:8}->{:8}  {} -> {}", name, r, lead, hue_name(hB), ax, bx);
        tiles.push((name.to_string(),ax,bx,lead.to_string(),r,hA,hB));
    }

    // ---- HTML ----
    let dock: String = tiles.iter().map(|t|
        format!("<span class=\"dockR\" style=\"background:linear-gradient(130deg,{} 0%,{} 75%)\">R</span>", t.1, t.2)
    ).collect();
    let cards: String = tiles.iter().map(|t|
        format!("<div class=\"card\">\
            <div class=\"glyph\" style=\"background:linear-gradient(130deg,{a} 0%,{b} 75%)\">R</div>\
            <div class=\"name\">{name}</div>\
            <div class=\"meta\">{lead} \u{2192} {lead2} \u{b7} +{rot:.0}\u{b0}</div>\
            <div class=\"sw\"><span style=\"background:{a}\"></span><span style=\"background:{b}\"></span></div>\
            <code>{a} \u{2192} {b}</code>\
            <code class=\"dim\">--logo-from-h:{hA:.0}; --logo-to-h:{hB:.0};</code>\
          </div>",
          a=t.1,b=t.2,name=t.0,lead=t.3,lead2=hue_name(t.6),rot=t.4,hA=t.5,hB=t.6)
    ).collect();

    let mut html = String::new();
    html.push_str(r#"<!doctype html><html><head><meta charset="utf-8"><title>Rusty* suite colorways</title>
<style>
  * { box-sizing:border-box; }
  body { margin:0; font:15px/1.5 -apple-system,Segoe UI,Roboto,sans-serif; color:#f8f8ff;
    background:linear-gradient(150deg,#1d2231 0%,#242b3c 55%,#2b3347 100%); min-height:100vh; padding:48px; }
  h1 { font-size:22px; font-weight:650; margin:0 0 4px; }
  h2 { font-size:13px; font-weight:600; letter-spacing:.08em; text-transform:uppercase; color:#c4c9e1; margin:44px 0 16px; }
  p.sub { color:#c4c9e1; margin:0 0 8px; max-width:800px; }
  .clip { -webkit-background-clip:text; background-clip:text; color:transparent; }
  .dock { display:inline-flex; gap:18px; align-items:center; padding:18px 26px; border-radius:26px;
    background:linear-gradient(180deg,rgba(35,41,59,.96),rgba(27,33,50,.96)); border:1px solid rgba(215,223,255,.14);
    box-shadow:0 20px 50px rgba(0,0,0,.35); }
  .dockR { font-size:46px; font-weight:680; line-height:1; -webkit-background-clip:text; background-clip:text; color:transparent; }
  .grid { display:grid; grid-template-columns:repeat(4,1fr); gap:16px; max-width:920px; }
  .card { background:rgba(39,45,64,.55); border:1px solid rgba(215,223,255,.12); border-radius:16px; padding:18px; }
  .glyph { font-size:52px; font-weight:680; line-height:1; -webkit-background-clip:text; background-clip:text; color:transparent; }
  .name { font-weight:600; margin-top:10px; text-transform:capitalize; }
  .meta { font-size:12px; color:#9aa0bf; text-transform:capitalize; }
  .sw { display:flex; gap:6px; margin:10px 0 8px; }
  .sw span { width:34px; height:14px; border-radius:4px; }
  code { display:block; font:11.5px/1.6 ui-monospace,Menlo,monospace; color:#ffc27a; }
  code.dim { color:#9aa0bf; }
</style></head><body>
"#);
    html.push_str("<h1>Rusty* suite \u{2014} one recipe, eight colorways</h1>\
        <p class=\"sub\">Each app reuses rustyfin's exact lightness/chroma and its split-complementary hue interval, rotated around the wheel. Locked L &amp; C means no icon out-shouts another \u{2014} distinct hues, coordinated set. rustyfin (+0\u{b0}) is its real gradient, untouched.</p>");
    html.push_str("<h2>The tray test \u{2014} docked side by side</h2><div class=\"dock\">");
    html.push_str(&dock);
    html.push_str("</div>");
    html.push_str("<h2>Per-app colorways</h2><div class=\"grid\">");
    html.push_str(&cards);
    html.push_str("</div></body></html>");
    std::fs::write("/tmp/rustyfin-suite-colorways.html", html).unwrap();
    println!("\nwrote /tmp/rustyfin-suite-colorways.html");
}

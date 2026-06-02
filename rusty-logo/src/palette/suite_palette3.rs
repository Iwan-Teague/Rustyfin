// Rusty* suite: curated wide-but-tasteful 2-stop gradients, all built on rustyfin's
// two L/C anchors (locked L & C => cohesive tray), hues hand-picked from well-liked
// regions, dodging the olive dead-zone and red+green clash. rustyfin kept exact.
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
    let c = if in_gamut(l,c,h) { c } else { max_chroma(l,h)*0.999 };
    let (r,g,b)=oklch_to_lin(l,c,h);
    let q=|v: f64| (lin_to_srgb(v)*255.0).round().clamp(0.0,255.0) as u8;
    format!("#{:02x}{:02x}{:02x}", q(r),q(g),q(b))
}

fn main() {
    let (la,ca,_)=rgb_to_oklch(255.0,145.0,77.0);  // rustyfin orange stop -> L,C
    let (lb,cb,_)=rgb_to_oklch(177.0,140.0,255.0); // rustyfin purple stop -> L,C

    // curated palette: (vibe, leadHue, accentHue). all in liked regions; none lead olive(95-135); no red+green.
    let board: [(&str,f64,f64);12] = [
        ("ember (rustyfin)", 50.0, 297.0),
        ("flame",            40.0, 330.0),
        ("coral",            20.0, 320.0),
        ("rose",            350.0, 290.0),
        ("orchid",          330.0, 270.0),
        ("plum",            300.0, 240.0),
        ("nebula",          270.0, 200.0),
        ("twilight",        245.0, 300.0),
        ("lagoon",          220.0, 160.0),
        ("aurora",          195.0, 290.0),
        ("mint",            175.0, 245.0),
        ("spring",          155.0, 250.0),
    ];

    println!("=== curated board (locked L/C; rustyfin recipe's lightness/chroma) ===");
    // (vibe, hexA, hexB, leadH, accH)
    let mut cells: Vec<(String,String,String,f64,f64)> = Vec::new();
    for (vibe,lh,ah) in board {
        let (ax,bx) = if (lh-50.0).abs()<0.1 && (ah-297.0).abs()<0.1 {
            ("#ff914d".to_string(),"#b18cff".to_string())   // rustyfin exact
        } else {
            (hex(la,ca,lh), hex(lb,cb,ah))
        };
        println!("  {:18} {} -> {}", vibe, ax, bx);
        cells.push((vibe.to_string(),ax,bx,lh,ah));
    }

    // proposed assignment for the 5 named apps (bool = reverse gradient direction)
    let pick = [("flame",true),("ember (rustyfin)",false),("twilight",true),("nebula",false),("lagoon",false)];
    let app_names = ["rustynet","rustyfin","rustychat","rustydns","rustytorrent"];
    // custom R: bowl returns toward stem but stops short, leg kicks out from there (open notch, no mid reconnect)
    // real Avenir Next Demi Bold R outline, bridge-to-stem re-routed into the bowl inner curve (open notch)
    let rmark = "<svg class=\"rmark\" viewBox=\"-6 -6 519 720\" aria-hidden=\"true\"><path fill=\"#fff\" d=\"M0 0 L246 0 Q296 0 343 10.5 Q390 21 426.5 45 Q463 69 485 108 Q507 147 507 203 Q507 280 463.5 328.5 Q420 377 345 394 L507 708 L382 708 L218 394 L218 305 Q256 305 283 301 Q310 297 331.5 286 Q353 275 367 254.5 Q381 234 381 202 Q381 173 368 154.5 Q355 136 335 125 Q315 114 289.5 110 Q264 106 240 106 L126 106 L126 708 L0 708 Z\"/></svg>";
    // original Avenir Next 600 R (text) for A/B toggle
    let rorig = "<span class=\"rorig\">R</span>";
    let dock: String = pick.iter().zip(app_names.iter()).map(|((vibe,rev),app)| {
        let c = cells.iter().find(|c| &c.0==vibe).unwrap();
        let (a,b) = if *rev { (&c.2,&c.1) } else { (&c.1,&c.2) };
        format!("<div class=\"dcol\"><div class=\"tile\" style=\"background:linear-gradient(130deg,{} 0%,{} 75%)\">{}{}</div><small>{}</small></div>", a, b, rmark, rorig, app)
    }).collect();

    let card = |c: &(String,String,String,f64,f64)| format!(
        "<div class=\"card\">\
           <div class=\"tile\" style=\"background:linear-gradient(130deg,{a} 0%,{b} 75%)\">{mark}{orig}</div>\
           <div class=\"name\">{vibe}</div>\
           <div class=\"sw\"><span style=\"background:{a}\"></span><span style=\"background:{b}\"></span></div>\
           <code>{a} \u{2192} {b}</code>\
         </div>", a=c.1,b=c.2,vibe=c.0,mark=rmark,orig=rorig);
    let cards: String = cells.iter().map(card).collect();

    let mut html = String::new();
    html.push_str(r#"<!doctype html><html><head><meta charset="utf-8"><title>Rusty* curated colorways</title>
<style>
  * { box-sizing:border-box; }
  body { margin:0; font:15px/1.5 -apple-system,Segoe UI,Roboto,sans-serif; color:#f8f8ff;
    background:linear-gradient(150deg,#1d2231 0%,#242b3c 55%,#2b3347 100%); min-height:100vh; padding:48px; }
  h1 { font-size:22px; font-weight:650; margin:0 0 4px; }
  h2 { font-size:13px; font-weight:600; letter-spacing:.08em; text-transform:uppercase; color:#c4c9e1; margin:44px 0 16px; }
  p.sub { color:#c4c9e1; margin:0 0 8px; max-width:820px; }
  .dock { display:inline-flex; gap:22px; align-items:flex-end; padding:20px 28px; border-radius:26px;
    background:linear-gradient(180deg,rgba(35,41,59,.96),rgba(27,33,50,.96)); border:1px solid rgba(215,223,255,.14);
    box-shadow:0 20px 50px rgba(0,0,0,.35); }
  .dcol { display:flex; flex-direction:column; align-items:center; gap:8px; }
  .dcol small { color:#9aa0bf; font-size:11px; }
  .tile { width:92px; height:92px; border-radius:23px; display:grid; place-items:center;
    box-shadow:0 6px 18px rgba(0,0,0,.3); }
  .tile > * { grid-area: 1 / 1; }
  .rmark { height:58px; width:auto; display:block; }
  .rorig { font-family:"Avenir Next","Segoe UI Variable Text","SF Pro Display",sans-serif;
    font-weight:600; font-size:80px; line-height:1; letter-spacing:0.01em; color:#fff; display:none; transform:translateY(3px); }
  body.show-orig .rmark { display:none; }
  body.show-orig .rorig { display:block; }
  .rtoggle { position:fixed; top:18px; right:18px; z-index:20; cursor:pointer;
    font:600 13px/1 -apple-system,Segoe UI,sans-serif; color:#fff;
    background:linear-gradient(130deg,#ff914d,#b18cff); border:none; padding:11px 15px; border-radius:11px;
    box-shadow:0 8px 22px rgba(0,0,0,.4); }
  .rtoggle:hover { filter:brightness(1.08); }
  .grid { display:grid; grid-template-columns:repeat(4,1fr); gap:16px; max-width:920px; }
  .card { background:rgba(39,45,64,.55); border:1px solid rgba(215,223,255,.12); border-radius:16px; padding:18px; }
  .name { font-weight:600; margin-top:10px; text-transform:capitalize; }
  .sw { display:flex; gap:6px; margin:10px 0 8px; }
  .sw span { width:34px; height:14px; border-radius:4px; }
  code { display:block; font:11.5px/1.6 ui-monospace,Menlo,monospace; color:#ffc27a; }
</style></head><body>
"#);
    html.push_str("<button class=\"rtoggle\" onclick=\"var b=document.body.classList.toggle('show-orig'); this.textContent=b?'Showing: Original Avenir R \u{2014} click for Custom':'Showing: Custom R \u{2014} click for Original';\">Showing: Custom R \u{2014} click for Original</button>");
    html.push_str("<h1>Rusty* suite \u{2014} curated colorways</h1>\
        <p class=\"sub\">Twelve tasteful 2-stop gradients, all built on rustyfin's lightness &amp; chroma (locked L/C = cohesive tray), hues hand-picked from well-liked regions \u{2014} no olive dead-zone, no red+green clash. Pick favourites and assign to apps.</p>");
    html.push_str("<h2>Proposed assignment \u{2014} the 5 named apps</h2><div class=\"dock\">");
    html.push_str(&dock);
    html.push_str("</div>");
    html.push_str("<h2>Full board \u{2014} 12 candidates</h2><div class=\"grid\">");
    html.push_str(&cards);
    html.push_str("</div></body></html>");
    std::fs::write("suite-curated.html", html).unwrap();
    println!("\nwrote suite-curated.html");
}

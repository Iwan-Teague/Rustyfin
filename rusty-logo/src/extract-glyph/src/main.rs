use ttf_parser::{Face, OutlineBuilder, Width};

struct B {
    ops: Vec<(char, Vec<f32>)>,
}
impl OutlineBuilder for B {
    fn move_to(&mut self, x: f32, y: f32) { self.ops.push(('M', vec![x, y])); }
    fn line_to(&mut self, x: f32, y: f32) { self.ops.push(('L', vec![x, y])); }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) { self.ops.push(('Q', vec![x1, y1, x, y])); }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.ops.push(('C', vec![x1, y1, x2, y2, x, y]));
    }
    fn close(&mut self) { self.ops.push(('Z', vec![])); }
}

fn main() {
    let data = std::fs::read("/System/Library/Fonts/Avenir Next.ttc").expect("read font");
    let n = ttf_parser::fonts_in_collection(&data).unwrap_or(1);

    let mut chosen: Option<u32> = None;
    for i in 0..n {
        if let Ok(face) = Face::parse(&data, i) {
            let w = face.weight().to_number();
            let italic = face.is_italic();
            let width = face.width();
            let mut sub = String::new();
            let names = face.names();
            for idx in 0..names.len() {
                if let Some(name) = names.get(idx) {
                    if name.name_id == 2 {
                        if let Some(s) = name.to_string() { sub = s; }
                    }
                }
            }
            eprintln!("face {:2} weight {:3} italic {:5} width {:?} sub {:?}", i, w, italic, width, sub);
            if w == 600 && !italic && width == Width::Normal && chosen.is_none() {
                chosen = Some(i);
            }
        }
    }
    let idx = chosen.expect("no weight-600 normal upright face found");
    eprintln!("\n==> chosen face index {}", idx);

    let face = Face::parse(&data, idx).unwrap();
    let upem = face.units_per_em();
    let gid = face.glyph_index('R').expect("no R glyph");
    let mut b = B { ops: Vec::new() };
    let bbox = face.outline_glyph(gid, &mut b).expect("no outline");
    let adv = face.glyph_hor_advance(gid).unwrap_or(0);
    eprintln!("upem {} advance {} bbox {:?}", upem, adv, bbox);

    // bbox of all coordinates (incl. control points)
    let (mut minx, mut miny, mut maxx, mut maxy) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for (_, c) in &b.ops {
        let mut k = 0;
        while k + 1 < c.len() {
            let (x, y) = (c[k], c[k + 1]);
            minx = minx.min(x); maxx = maxx.max(x);
            miny = miny.min(y); maxy = maxy.max(y);
            k += 2;
        }
    }
    let pad = 0.0_f32;
    // flip Y (font y-up -> svg y-down), translate to origin
    let fx = |x: f32| (x - minx + pad);
    let fy = |y: f32| (maxy - y + pad);
    let mut d = String::new();
    for (cmd, c) in &b.ops {
        d.push(*cmd);
        let mut k = 0;
        while k + 1 < c.len() {
            d.push_str(&format!(" {:.1} {:.1}", fx(c[k]), fy(c[k + 1])));
            k += 2;
        }
        d.push(' ');
    }
    let vbw = maxx - minx + 2.0 * pad;
    let vbh = maxy - miny + 2.0 * pad;
    println!("VIEWBOX 0 0 {:.1} {:.1}", vbw, vbh);
    println!("ADV_SCALED {:.1}", adv as f32 - minx);
    println!("PATH {}", d.trim());

    // raw font-unit points list (for locating the junction during notch surgery)
    eprintln!("\n--- raw move/line anchor points (font units, y-up) ---");
    for (cmd, c) in &b.ops {
        if *cmd == 'M' || *cmd == 'L' {
            eprintln!("{} {:.0} {:.0}", cmd, c[0], c[1]);
        } else if *cmd == 'Q' {
            eprintln!("Q ctrl {:.0} {:.0} -> {:.0} {:.0}", c[0], c[1], c[2], c[3]);
        } else if *cmd == 'C' {
            eprintln!("C .. -> {:.0} {:.0}", c[4], c[5]);
        }
    }
}

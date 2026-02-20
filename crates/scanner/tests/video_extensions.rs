use rustfin_scanner::parser::is_video_file;

#[test]
fn recognizes_common_video_extensions() {
    for name in [
        "a.mp4", "b.MKV", "c.mov", "d.m2ts", "e.webm", "f.avi", "g.mpeg", "h.ts", "i.m4v", "j.WMV",
        "k.flv", "l.3gp", "m.ogv", "n.vob", "o.mxf", "p.f4v", "q.3g2", "r.mts", "s.asf", "t.mpe",
        "u.mpv",
    ] {
        assert!(is_video_file(name), "should detect {name}");
    }
}

#[test]
fn rejects_non_video_files() {
    for name in [
        "notes.txt",
        "poster.jpg",
        "subs.srt",
        "metadata.nfo",
        "archive.zip",
    ] {
        assert!(!is_video_file(name), "should NOT detect {name}");
    }
}

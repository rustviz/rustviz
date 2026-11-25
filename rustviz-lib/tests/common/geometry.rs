//! Geometric invariants: properties of the drawing itself, checked on
//! every snippet alongside the ownership invariants in `invariants.rs`
//! (they share its `KNOWN_INVARIANT_FAILURES` list and its test,
//! `corpus_invariants_hold`).
//!
//! The ownership checks read what the picture says; these read where it
//! is drawn. A segment that runs past a fork dot, a column that starts or
//! stops in mid-air, two arrows along one stretch, two dots on one spot:
//! the text of every tooltip can be right and the picture still wrong,
//! and no tooltip assertion notices (#1278's `else if` stub was found by
//! eye).
//!
//! A column is its `data-hash`. Its segments are the `line` and
//! `branch_line` elements, each reduced to its centre line: a hollow
//! segment's two sides (`side1` / `side2`, or a main column's two
//! `<line>`s) are averaged, the bevels and hover polygons ignored.
//! Positions are compared to `TOL` px: the plugin computes every shared
//! point with the same arithmetic, so a real joint matches far closer.

use std::collections::BTreeMap;

/// The geometric invariants' names (described in `invariants::INVARIANTS`).
pub const INVARIANTS: &[&str] = &[
    "line-starts-nowhere", "line-ends-nowhere", "line-through-fork", "arrows-overlap",
    "marks-on-one-spot", "arrow-misses-target", "outside-figure", "heads-or-logos-overlap",
    "shaft-through-mark", "arrows-end-to-end",
    "fork-without-dot", "arrow-along-leg",
];

/// How far apart two points the drawing means to be one may be.
const TOL: f64 = 1.0;
/// Two arrows may share this much of a straight stretch: where a short
/// arrow's elbow meets another at a column (a one-line function's
/// parameter coming in and its result going out) they touch for up to
/// 10 px. An arrow head is 12.75 px long.
const OVERLAP_PX: f64 = 12.0;
/// Half a shaft's stroke (5 px wide).
const SHAFT_HALF: f64 = 2.5;
/// An arrow's head stops this close to the target dot's centre (8.2 px
/// for a diagonal one).
const HEAD_REACH: f64 = 10.0;
/// An arrow's head may lie over this much of a column's slanted leg's
/// centre line (`arrow-along-leg`): a leg ending at the dot the head
/// points at, or crossing the head, passes under it for a few px.
const HEAD_ON_LEG_PX: f64 = 6.0;

#[derive(Debug, Clone)]
struct Seg {
    id: String,
    var: String,
    hash: String,
    /// A main column's `line` (not a conditional's arm).
    main: bool,
    a: (f64, f64),
    b: (f64, f64),
}

#[derive(Debug, Clone)]
struct Dot {
    id: String,
    var: String,
    hash: String,
    x: f64,
    y: f64,
    events: Vec<String>,
    tip: String,
}

#[derive(Debug, Clone)]
struct Arrow {
    id: String,
    var: String,
    to: String,
    line: String,
    tip: String,
    /// The shaft's straight pieces (a hop's arc left out).
    pieces: Vec<((f64, f64), (f64, f64))>,
    /// The centres of the dots its hops go over.
    hops: Vec<(f64, f64)>,
    head: Option<(f64, f64)>,
    /// The head's triangle.
    tri: Vec<(f64, f64)>,
}

/// A function logo (`fn`).
#[derive(Debug, Clone)]
struct Logo {
    id: String,
    var: String,
    to: String,
    line: String,
    x: f64,
    y: f64,
    tip: String,
}

#[derive(Debug, Clone, Default)]
pub struct Geo {
    segs: Vec<Seg>,
    dots: Vec<Dot>,
    arrows: Vec<Arrow>,
    logos: Vec<Logo>,
    /// Stop bars, by column: (hash, centre).
    stops: Vec<(String, (f64, f64))>,
    headers: Vec<String>,
    /// The `<svg>`'s width and height.
    size: (f64, f64),
    /// Every coordinate a hoverable element is drawn at: (id, x or y, value).
    coords: Vec<(String, char, f64)>,
}

fn num(n: roxmltree::Node, a: &str) -> f64 {
    n.attribute(a).and_then(|v| v.trim_end_matches("px").parse().ok()).unwrap_or(f64::NAN)
}

fn ends(n: roxmltree::Node) -> ((f64, f64), (f64, f64)) {
    ((num(n, "x1"), num(n, "y1")), (num(n, "x2"), num(n, "y2")))
}

fn mid(p: (f64, f64), q: (f64, f64)) -> (f64, f64) {
    ((p.0 + q.0) / 2.0, (p.1 + q.1) / 2.0)
}

/// The points of `points="x y x y"` (or `x,y x,y`).
fn points(s: &str) -> Vec<(f64, f64)> {
    let n: Vec<f64> = s.split(|c: char| c == ',' || c.is_whitespace())
        .filter(|t| !t.is_empty()).filter_map(|t| t.parse().ok()).collect();
    n.chunks_exact(2).map(|c| (c[0], c[1])).collect()
}

/// The straight pieces of a hop arrow's `d` (`M x y L x y A rx ry r l s x y
/// L x y …`, absolute commands only): the arcs hop over a column and are
/// left out.
fn path_pieces(d: &str) -> Vec<((f64, f64), (f64, f64))> {
    path_parts(d).0
}

/// A hop arrow's straight pieces, and the midpoints of its arcs' chords
/// (the dots it hops over).
fn path_parts(d: &str) -> (Vec<((f64, f64), (f64, f64))>, Vec<(f64, f64)>) {
    let toks: Vec<&str> = d.split(|c: char| c == ',' || c.is_whitespace()).filter(|t| !t.is_empty()).collect();
    let f = |i: usize| toks.get(i).and_then(|t| t.parse::<f64>().ok());
    let mut out = Vec::new();
    let mut hops = Vec::new();
    let mut at: Option<(f64, f64)> = None;
    let mut i = 0;
    while i < toks.len() {
        let (n, to) = match toks[i] {
            "M" | "L" => (3, f(i + 1).zip(f(i + 2))),
            "A" => (8, f(i + 6).zip(f(i + 7))),
            _ => break,
        };
        let Some(to) = to else { break };
        if let Some(from) = at {
            match toks[i] {
                "L" => out.push((from, to)),
                "A" => hops.push(mid(from, to)),
                _ => {}
            }
        }
        at = Some(to);
        i += n;
    }
    (out, hops)
}

pub fn parse(doc: &roxmltree::Document) -> Geo {
    let root = doc.root_element();
    let mut g = Geo { size: (num(root, "width"), num(root, "height")), ..Default::default() };
    // A conditional arm's segment: its parts, by element id.
    let mut arms: BTreeMap<String, (usize, Seg, BTreeMap<String, ((f64, f64), (f64, f64))>)> = BTreeMap::new();
    for n in doc.descendants().filter(|n| n.attribute("data-rv-id").is_some()) {
        let a = |k: &str| n.attribute(k).unwrap_or("").to_owned();
        let id = a("data-rv-id");
        let base = id.split('/').next().unwrap_or("").to_owned();
        let hash = n.attribute("data-hash")
            .or_else(|| n.descendants().find_map(|c| c.attribute("data-hash"))).unwrap_or("").to_owned();
        for c in n.descendants() {
            for (k, axis) in [("x", 'x'), ("cx", 'x'), ("x1", 'x'), ("x2", 'x'), ("y", 'y'), ("cy", 'y'), ("y1", 'y'), ("y2", 'y')] {
                if let Some(v) = c.attribute(k).and_then(|v| v.parse::<f64>().ok()) {
                    g.coords.push((id.clone(), axis, v));
                }
            }
        }
        match n.attribute("data-rv-kind").unwrap_or("") {
            "dot" => {
                let Some(c) = n.descendants().find(|c| c.has_tag_name("circle")) else { continue };
                g.dots.push(Dot {
                    id, var: a("data-rv-var"), hash, x: num(c, "cx"), y: num(c, "cy"),
                    events: a("data-rv-event").split(' ').filter(|e| !e.is_empty()).map(str::to_owned).collect(),
                    tip: a("data-tooltip-text"),
                });
            }
            "line" => {
                let ls: Vec<_> = n.descendants().filter(|c| c.has_tag_name("line")).map(ends).collect();
                let (p, q) = match ls.as_slice() {
                    [l] => *l,
                    [l, m] => (mid(l.0, m.0), mid(l.1, m.1)),
                    _ => continue,
                };
                g.segs.push(Seg { id, var: a("data-rv-var"), hash, main: true, a: p, b: q });
            }
            "branch_line" => {
                let Some(part @ ("line" | "side1" | "side2")) = n.attribute("data-rv-part") else { continue };
                let order = arms.len();
                let e = arms.entry(base.clone()).or_insert_with(|| (order, Seg {
                    id: base, var: a("data-rv-var"), hash, main: false, a: (0.0, 0.0), b: (0.0, 0.0),
                }, BTreeMap::new()));
                e.2.insert(part.to_owned(), ends(n));
            }
            "stop_bar" if n.attribute("data-rv-part").is_none() => {
                let ((x1, y1), (x2, y2)) = ends(n);
                g.stops.push((hash, mid((x1, y1), (x2, y2))));
            }
            "arrow" => {
                let mut pieces = Vec::new();
                let mut hops = Vec::new();
                for c in n.descendants().filter(|c| c.attribute("class").is_some_and(|k| k.split(' ').any(|k| k == "arrow-shaft"))) {
                    if c.has_tag_name("polyline") {
                        let p = points(c.attribute("points").unwrap_or(""));
                        pieces.extend(p.windows(2).map(|w| (w[0], w[1])));
                    } else {
                        let (p, h) = path_parts(c.attribute("d").unwrap_or(""));
                        pieces.extend(p);
                        hops.extend(h);
                    }
                }
                let tri = n.descendants()
                    .find(|c| c.attribute("class").is_some_and(|k| k.split(' ').any(|k| k == "arrow-head")))
                    .map(|c| points(c.attribute("points").unwrap_or(""))).unwrap_or_default();
                let head = tri.get(1).copied();
                g.arrows.push(Arrow {
                    id, var: a("data-rv-var"), to: a("data-rv-to"), line: a("data-rv-line"),
                    tip: a("data-tooltip-text"), pieces, hops, head, tri,
                });
            }
            "fn" => g.logos.push(Logo {
                id, var: a("data-rv-var"), to: a("data-rv-to"), line: a("data-rv-line"),
                x: num(n, "x"), y: num(n, "y"), tip: a("data-tooltip-text"),
            }),
            "header" => g.headers.push(a("data-rv-var")),
            _ => {}
        }
    }
    let mut arms: Vec<_> = arms.into_values().collect();
    arms.sort_by_key(|(order, _, _)| *order);
    for (_, mut seg, parts) in arms {
        let (p, q) = match (parts.get("line"), parts.get("side1"), parts.get("side2")) {
            (Some(l), _, _) => *l,
            (None, Some(s), Some(t)) => (mid(s.0, t.0), mid(s.1, t.1)),
            _ => continue,
        };
        seg.a = p;
        seg.b = q;
        g.segs.push(seg);
    }
    g
}

fn near(p: (f64, f64), q: (f64, f64)) -> bool {
    (p.0 - q.0).abs() <= TOL && (p.1 - q.1).abs() <= TOL
}

fn len(a: (f64, f64), b: (f64, f64)) -> f64 {
    (b.0 - a.0).hypot(b.1 - a.1)
}

/// Where `p` lies along the segment a–b, in px from a, if it's within
/// `tol` of the segment's line.
fn along(p: (f64, f64), a: (f64, f64), b: (f64, f64), tol: f64) -> Option<f64> {
    let l = len(a, b);
    if l < 1e-9 {
        return None;
    }
    let (dx, dy) = ((b.0 - a.0) / l, (b.1 - a.1) / l);
    let off = ((p.0 - a.0) * dy - (p.1 - a.1) * dx).abs();
    (off <= tol).then(|| (p.0 - a.0) * dx + (p.1 - a.1) * dy)
}

/// Whether the segment a–b lies within the segment c–d (a stretch drawn
/// twice, like an arm's own row over the rest of its column, #845).
fn within(a: (f64, f64), b: (f64, f64), c: (f64, f64), d: (f64, f64)) -> bool {
    let l = len(c, d);
    [a, b].iter().all(|&p| along(p, c, d, TOL).is_some_and(|t| t >= -TOL && t <= l + TOL))
}

/// How far `c` is from the segment p–q.
fn off_piece(c: (f64, f64), p: (f64, f64), q: (f64, f64)) -> f64 {
    let l = len(p, q);
    let t = if l < 1e-9 { 0.0 } else { (((c.0 - p.0) * (q.0 - p.0) + (c.1 - p.1) * (q.1 - p.1)) / (l * l)).clamp(0.0, 1.0) };
    len(c, (p.0 + (q.0 - p.0) * t, p.1 + (q.1 - p.1) * t))
}

type Pt = (f64, f64);

/// How long a stretch of the piece p–q has the piece r–s's centre line
/// within `SHAFT_HALF` of its own (and alongside r–s, not past its ends):
/// the two 5 px strokes lie at least half on each other there. Two pieces
/// on one track share their common stretch; two crossing at an angle
/// share 5 px / sin(angle) or less (12 px at 25 degrees).
fn shared_stretch(a: (Pt, Pt), b: (Pt, Pt)) -> f64 {
    stretch_within(a, b, SHAFT_HALF)
}

/// `shared_stretch` with the centre lines within `half` px.
fn stretch_within((p, q): (Pt, Pt), (r, s): (Pt, Pt), half: f64) -> f64 {
    let (lp, lr) = (len(p, q), len(r, s));
    if lp < TOL || lr < TOL {
        return 0.0;
    }
    // A point p + t (q - p) / lp: its signed distance off r–s's line and
    // its place along r–s are both linear in t.
    let (dx, dy) = ((q.0 - p.0) / lp, (q.1 - p.1) / lp);
    let (ex, ey) = ((s.0 - r.0) / lr, (s.1 - r.1) / lr);
    let (wx, wy) = (p.0 - r.0, p.1 - r.1);
    let off = (wx * ey - wy * ex, dx * ey - dy * ex);
    let at = (wx * ex + wy * ey, dx * ex + dy * ey);
    // The t where a + b t lies in lo..=hi.
    let between = |(a, b): (f64, f64), lo: f64, hi: f64| -> (f64, f64) {
        if b.abs() < 1e-12 {
            return if a >= lo && a <= hi { (f64::NEG_INFINITY, f64::INFINITY) } else { (1.0, 0.0) };
        }
        let (t1, t2) = ((lo - a) / b, (hi - a) / b);
        (t1.min(t2), t1.max(t2))
    };
    let (o1, o2) = between(off, -half, half);
    let (a1, a2) = between(at, 0.0, lr);
    (o2.min(a2).min(lp) - o1.max(a1).max(0.0)).max(0.0)
}

fn is_fork_or_join(d: &Dot) -> bool {
    d.events.iter().any(|e| e == "branch" || e == "branch_join")
}

/// A field's column and its struct's (`p` and `p.name`, `p.name` and
/// `p.name.0`).
fn family(a: &str, b: &str) -> bool {
    a != b && (a.strip_prefix(b).is_some_and(|r| r.starts_with('.')) || b.strip_prefix(a).is_some_and(|r| r.starts_with('.')))
}

/// Every geometric violation in one timeline: (invariant, what and where).
pub fn check(g: &Geo) -> Vec<(&'static str, String)> {
    let mut out = Vec::new();
    let fmt = |p: (f64, f64)| format!("({:.1}, {:.1})", p.0, p.1);

    for s in &g.segs {
        if len(s.a, s.b) < TOL {
            continue;
        }
        let col = || g.segs.iter().filter(|o| o.hash == s.hash);
        // Drawn twice: the longer segment's ends are checked.
        if col().any(|o| !std::ptr::eq(o, s) && len(o.a, o.b) > len(s.a, s.b) + TOL && within(s.a, s.b, o.a, o.b)) {
            continue;
        }
        let (top, bottom) = if s.a.1 <= s.b.1 { (s.a, s.b) } else { (s.b, s.a) };
        for (end, inv) in [(top, "line-starts-nowhere"), (bottom, "line-ends-nowhere")] {
            let anchored = g.dots.iter().any(|d| d.hash == s.hash && near(end, (d.x, d.y)))
                || col().any(|o| !std::ptr::eq(o, s) && (near(end, o.a) || near(end, o.b)))
                || g.stops.iter().any(|(h, p)| *h == s.hash && near(end, *p))
                // A struct's column stops on the row its field is moved out
                // (and starts again where the field gets a value back): the
                // event is on the field's dot.
                || (s.main && g.dots.iter().any(|d| (d.y - end.1).abs() <= TOL && family(&d.var, &s.var)));
            if !anchored {
                out.push((inv, format!("{} at {}: no dot or other segment of {} there", s.id, fmt(end), s.var)));
            }
        }
    }

    // A segment that runs through a fork or join dot and on past it, instead
    // of ending there (#1278's `else if` stub).
    for d in g.dots.iter().filter(|d| is_fork_or_join(d)) {
        for s in g.segs.iter().filter(|s| s.hash == d.hash) {
            let l = len(s.a, s.b);
            if let Some(t) = along((d.x, d.y), s.a, s.b, TOL) {
                if t > 2.0 && t < l - 2.0 {
                    out.push(("line-through-fork",
                        format!("{} runs from {} to {}, through {} at {}", s.id, fmt(s.a), fmt(s.b), d.id, fmt((d.x, d.y)))));
                }
            }
        }
    }

    // Two or more legs of one column leaving a point downwards in different
    // directions (a fork), or arriving at it from above (a join), meet at a
    // dot: the conditional's split or join. Without one the diamond starts
    // or ends in mid-line (#1371: a column mutably lent out across an
    // `if let` forked and joined with no dots).
    let mut forks: Vec<(String, (f64, f64))> = Vec::new();
    for s in g.segs.iter().filter(|s| len(s.a, s.b) >= TOL) {
        for (end, other) in [(s.a, s.b), (s.b, s.a)] {
            let down = other.1 > end.1 + TOL;
            let up = other.1 < end.1 - TOL;
            if !down && !up {
                continue;
            }
            let dir = |p: (f64, f64), q: (f64, f64)| { let l = len(p, q); ((q.0 - p.0) / l, (q.1 - p.1) / l) };
            let d1 = dir(end, other);
            let splits = g.segs.iter().filter(|o| o.hash == s.hash && !std::ptr::eq(*o, s) && len(o.a, o.b) >= TOL)
                .filter_map(|o| if near(end, o.a) { Some(o.b) } else if near(end, o.b) { Some(o.a) } else { None })
                .any(|q| {
                    let same_side = if down { q.1 > end.1 + TOL } else { q.1 < end.1 - TOL };
                    let d2 = dir(end, q);
                    same_side && (d1.0 * d2.1 - d1.1 * d2.0).abs() > 0.05
                });
            if splits && !g.dots.iter().any(|d| d.hash == s.hash && near(end, (d.x, d.y)))
                && !forks.iter().any(|(h, p)| *h == s.hash && near(*p, end)) {
                forks.push((s.hash.clone(), end));
                out.push(("fork-without-dot", format!("{} legs {} at {}, where {} has no dot",
                    s.var, if down { "fork" } else { "join" }, fmt(end), s.var)));
            }
        }
    }

    // Two arrows along one stretch: the pointer reaches only the one drawn
    // on top (#1264, #1187). On one track, or one slanting across the
    // other's track at a shallow angle, their centre lines within half a
    // stroke of each other (#1468: a level return and a binding arrow
    // rising 28 px over 330 px lay on each other for 47 px; two returns
    // leaving one dot 6 degrees apart, for 19 px).
    for (i, a) in g.arrows.iter().enumerate() {
        for b in &g.arrows[i + 1..] {
            if a.tip == b.tip {
                continue;
            }
            let mut most: f64 = 0.0;
            for &(p, q) in &a.pieces {
                for &(r, s) in &b.pieces {
                    most = most.max(shared_stretch((p, q), (r, s)));
                }
            }
            if most > OVERLAP_PX {
                out.push(("arrows-overlap", format!("{} and {} share {:.0} px", a.id, b.id, most)));
            }
        }
    }

    // Two marks on one spot with different tooltips: only one can be hovered.
    for (i, d) in g.dots.iter().enumerate() {
        for e in &g.dots[i + 1..] {
            if d.hash == e.hash && (d.x - e.x).abs() < 2.0 && (d.y - e.y).abs() < 2.0 && d.tip != e.tip {
                out.push(("marks-on-one-spot", format!("{} and {} at {}", d.id, e.id, fmt((d.x, d.y)))));
            }
        }
    }
    for (i, f) in g.logos.iter().enumerate() {
        for e in &g.logos[i + 1..] {
            if (f.x - e.x).abs() < 2.0 && (f.y - e.y).abs() < 2.0 && f.tip != e.tip {
                out.push(("marks-on-one-spot", format!("{} and {} at {}", f.id, e.id, fmt((f.x, f.y)))));
            }
        }
    }

    // An arrow to a column ends at one of its dots. (Not an arrow to a
    // function's logo, nor one to a static or literal drawn beside its own
    // column, `s>s`.)
    for a in &g.arrows {
        let Some(h) = a.head else { continue };
        if a.var == a.to || !g.headers.contains(&a.to)
            || g.logos.iter().any(|l| l.var == a.var && l.to == a.to && l.line == a.line) {
            continue;
        }
        if !g.dots.iter().any(|d| d.var == a.to && len(h, (d.x, d.y)) <= HEAD_REACH) {
            out.push(("arrow-misses-target", format!("{} ends at {}, at no dot of {}", a.id, fmt(h), a.to)));
        }
    }

    // An arrow's head on another arrow (its shaft or head), or a logo on
    // an arrow other than its own or on another logo (#1307): under
    // `OVERLAP_PX` of shared shaft, so `arrows-overlap` misses them.
    let samples = |pieces: &[((f64, f64), (f64, f64))]| -> Vec<(f64, f64)> {
        let mut v = Vec::new();
        for &(p, q) in pieces {
            let n = (len(p, q).ceil() as usize).max(1);
            for k in 0..=n {
                let t = k as f64 / n as f64;
                v.push((p.0 + (q.0 - p.0) * t, p.1 + (q.1 - p.1) * t));
            }
        }
        v
    };
    let tri_edges = |t: &[(f64, f64)]| -> Vec<((f64, f64), (f64, f64))> {
        if t.len() < 3 { return Vec::new() }
        vec![(t[0], t[1]), (t[1], t[2]), (t[2], t[0])]
    };
    // Signed distance of p inside triangle t (positive inside).
    let inside = |t: &[(f64, f64)], p: (f64, f64)| -> f64 {
        if t.len() < 3 { return f64::NEG_INFINITY }
        let area = (t[1].0 - t[0].0) * (t[2].1 - t[0].1) - (t[2].0 - t[0].0) * (t[1].1 - t[0].1);
        let s = area.signum();
        tri_edges(t).iter().map(|&(a, b)| {
            let l = len(a, b).max(1e-9);
            s * ((b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0)) / l
        }).fold(f64::INFINITY, f64::min)
    };
    // How far p is from triangle t (0 inside it).
    let off_tri = |t: &[(f64, f64)], p: (f64, f64)| -> f64 {
        if inside(t, p) >= 0.0 { return 0.0 }
        tri_edges(t).iter().map(|&(a, b)| {
            let l = len(a, b).max(1e-9);
            let u = (((p.0 - a.0) * (b.0 - a.0) + (p.1 - a.1) * (b.1 - a.1)) / (l * l)).clamp(0.0, 1.0);
            len(p, (a.0 + (b.0 - a.0) * u, a.1 + (b.1 - a.1) * u))
        }).fold(f64::INFINITY, f64::min)
    };
    // A logo's box: the italic `f` at 20px, from its text anchor.
    let logo_box = |l: &Logo| (l.x - 1.0, l.y - 15.0, l.x + 11.0, l.y + 3.0);
    let in_box = |b: (f64, f64, f64, f64), p: (f64, f64), m: f64| p.0 > b.0 - m && p.0 < b.2 + m && p.1 > b.1 - m && p.1 < b.3 + m;
    let mut hl: Vec<String> = Vec::new();
    for a in &g.arrows {
        for b in &g.arrows {
            // A one-line function's parameter arriving and its value
            // returned share the stub right of its dot by design.
            let elbow = (a.var == "caller" && b.to == "caller" && a.to == b.var)
                || (b.var == "caller" && a.to == "caller" && b.to == a.var);
            if std::ptr::eq(a, b) || a.tip == b.tip || a.tri.len() < 3 || (elbow && a.line == b.line) {
                continue;
            }
            // b's shaft (2.5 px either side of its centre line) or head in a's head.
            // A shaft that starts or ends at the head's column, or hops over
            // the dot the head points at, can't keep clear of it there: it
            // may come up to the head. One passing by may not touch it (its
            // edge within 1 px of the triangle, #1316's `y -> a` grazing
            // `a -> z`'s head).
            let related = b.var == a.to || b.to == a.to
                || a.head.is_some_and(|h| b.hops.iter().any(|&c| len(c, h) < 8.0));
            let shaft = samples(&b.pieces).into_iter()
                .any(|p| inside(&a.tri, p) > -1.5 || (!related && off_tri(&a.tri, p) < SHAFT_HALF + 1.0));
            let head = b.tri.len() == 3 && std::ptr::addr_of!(*a) < std::ptr::addr_of!(*b)
                && samples(&tri_edges(&b.tri)).into_iter().any(|p| inside(&a.tri, p) > 1.0);
            if shaft || head {
                hl.push(format!("head of {} on {} of {}", a.id, if shaft { "the shaft" } else { "the head" }, b.id));
            }
        }
    }
    for (i, l) in g.logos.iter().enumerate() {
        let bx = logo_box(l);
        for b in &g.arrows {
            if b.var == l.var && b.to == l.to && b.line == l.line {
                continue;
            }
            if samples(&b.pieces).into_iter().any(|p| in_box(bx, p, 1.5))
                || samples(&tri_edges(&b.tri)).into_iter().any(|p| in_box(bx, p, -1.0)) {
                hl.push(format!("logo {} on {}", l.id, b.id));
            }
        }
        for m in &g.logos[i + 1..] {
            let c = logo_box(m);
            if l.tip != m.tip && bx.0.max(c.0) < bx.2.min(c.2) - 1.0 && bx.1.max(c.1) < bx.3.min(c.3) - 1.0 {
                hl.push(format!("logo {} on logo {}", l.id, m.id));
            }
        }
    }
    out.extend(hl.into_iter().map(|w| ("heads-or-logos-overlap", w)));

    // An arrow's head or shaft lying along a column's slanted leg, into
    // or out of a conditional's arm (#1476: a return coming down into a
    // lender's `else` drop dot at about the angle of the lender's own
    // `else` leg hid the leg's last 10 px under its head). A head: the
    // leg's centre line within 1 px of the triangle for more than
    // `HEAD_ON_LEG_PX` (a leg merely ending at the dot the head points at,
    // or crossing the head, is covered for a few px at most). A shaft:
    // see below. (Not a
    // vertical stretch of a column: a head coming up into a dot from a
    // track below lies on the column under the dot, as it does on main in
    // 100-odd snippets, #1488.)
    for a in &g.arrows {
        for s in g.segs.iter().filter(|s| (s.a.0 - s.b.0).abs() > TOL) {
            let covered = if a.tri.len() == 3 {
                samples(&[(s.a, s.b)]).into_iter().filter(|&p| inside(&a.tri, p) > -1.0).count()
            } else {
                0
            };
            // A shaft: centre lines within half a stroke for more than
            // `OVERLAP_PX`, as two arrows'; or, leaving (or reaching) the
            // dot the leg ends at less than 30 degrees off it, the 5 px
            // stroke on the leg's (~5 px) for more than 8 px (a borrow
            // leaving a join dot steeply beside the leg into it).
            let sin = |(p, q): (Pt, Pt)| ((q.0 - p.0) * (s.b.1 - s.a.1) - (q.1 - p.1) * (s.b.0 - s.a.0)).abs()
                / (len(p, q) * len(s.a, s.b)).max(1e-9);
            let at_its_dot = |(p, q): (Pt, Pt)| [p, q].iter().any(|&e| len(e, s.a) < 8.0 || len(e, s.b) < 8.0);
            let along = a.pieces.iter().map(|&p| shared_stretch(p, (s.a, s.b))).fold(0.0, f64::max);
            let beside = a.pieces.iter().filter(|&&p| len(p.0, p.1) >= TOL && sin(p) < 0.5 && at_its_dot(p))
                .map(|&p| stretch_within(p, (s.a, s.b), 4.5)).fold(0.0, f64::max);
            if covered as f64 > HEAD_ON_LEG_PX {
                out.push(("arrow-along-leg", format!("head of {} lies along {} for {} px", a.id, s.id, covered)));
            } else if along > OVERLAP_PX || beside > 8.0 {
                out.push(("arrow-along-leg", format!("{} runs along {} for {:.0} px", a.id, s.id, along.max(beside))));
            }
        }
    }

    // A shaft through a dot's centre, or along a stop bar, with no hop
    // (#1344): the mark reads as part of the arrow (a column it doesn't
    // touch looks moved or lent), and the arrow, drawn on top, takes its
    // hover. An arrow's own ends are 5.25 px or more off its dots'
    // centres, so they never count.
    for a in &g.arrows {
        for d in &g.dots {
            if a.pieces.iter().any(|&(p, q)| off_piece((d.x, d.y), p, q) < SHAFT_HALF) {
                out.push(("shaft-through-mark", format!("{} runs through {} at {}", a.id, d.id, fmt((d.x, d.y)))));
            }
        }
        for (h, c) in &g.stops {
            if a.pieces.iter().any(|&(p, q)| (p.1 - q.1).abs() < TOL && (p.1 - c.1).abs() < TOL
                && c.0 > p.0.min(q.0) && c.0 < p.0.max(q.0)) {
                out.push(("shaft-through-mark", format!("{} runs along the stop bar of column {h} at {}", a.id, fmt(*c))));
            }
        }
    }

    // Two arrows between columns end to end on one line: one's head
    // points at a dot and the other leaves that dot straight on in the
    // same direction, so the two read as one arrow through the dot
    // (#1349, #577). (Not a call's short arrow to or from its logo: the
    // logo marks where it starts or ends.)
    let between_columns = |a: &Arrow| a.var != a.to && g.headers.contains(&a.var) && g.headers.contains(&a.to);
    for a in g.arrows.iter().filter(|a| between_columns(a)) {
        let (Some(&(p, q)), Some(tip)) = (a.pieces.last(), a.head) else { continue };
        for b in g.arrows.iter().filter(|a| between_columns(a)) {
            let Some(&(r, s)) = b.pieces.first() else { continue };
            if std::ptr::eq(a, b) || a.tip == b.tip || len(p, q) < TOL || len(r, s) < TOL {
                continue;
            }
            let cos = ((q.0 - p.0) * (s.0 - r.0) + (q.1 - p.1) * (s.1 - r.1)) / (len(p, q) * len(r, s));
            let beyond = along(r, p, q, TOL).map(|t| t - len(p, q));
            if cos > 0.99 && beyond.is_some_and(|t| t > 0.0) && len(tip, r) < 15.0 {
                out.push(("arrows-end-to-end", format!("{} runs on from the head of {} at {}", b.id, a.id, fmt(r))));
            }
        }
    }

    // Inside the figure.
    let (w, ht) = g.size;
    if w.is_finite() && ht.is_finite() {
        for (id, axis, v) in &g.coords {
            let lim = if *axis == 'x' { w } else { ht };
            if *v < -0.5 || *v > lim + 0.5 {
                out.push(("outside-figure", format!("{id}: {axis} = {v} outside 0..{lim}")));
            }
        }
    }
    out
}

#[cfg(test)]
mod unit {
    use super::*;

    fn invs(body: &str) -> Vec<&'static str> {
        let svg = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"500px\" height=\"500px\">{body}</svg>");
        let doc = roxmltree::Document::parse(&svg).unwrap();
        let mut v: Vec<_> = check(&parse(&doc)).into_iter().map(|(i, _)| i).collect();
        v.sort();
        v.dedup();
        v
    }
    fn dot(var: &str, x: f64, y: f64, ev: &str) -> String {
        format!("<circle cx=\"{x}\" cy=\"{y}\" r=\"5\" data-hash=\"{var}\" data-tooltip-text=\"{ev} {x} {y}\" \
                 data-rv-id=\"{var}@{x},{y}:dot:{ev}\" data-rv-kind=\"dot\" data-rv-event=\"{ev}\" data-rv-var=\"{var}\"/>")
    }
    /// A main column's hollow segment (two side lines).
    fn line(var: &str, x: f64, y1: f64, y2: f64) -> String {
        format!("<g data-rv-id=\"{var}@{y1}:line\" data-rv-kind=\"line\" data-rv-var=\"{var}\">\
                 <line data-hash=\"{var}\" x1=\"{}\" y1=\"{y1}\" x2=\"{}\" y2=\"{y2}\"/>\
                 <line data-hash=\"{var}\" x1=\"{}\" y1=\"{y1}\" x2=\"{}\" y2=\"{y2}\"/></g>",
                x - 1.75, x - 1.75, x + 1.75, x + 1.75)
    }
    /// A conditional arm's segment: hollow sides 1.75 px either side of
    /// the centre line, plus its hover polygon.
    fn arm(var: &str, n: u32, a: (f64, f64), b: (f64, f64)) -> String {
        let l = len(a, b);
        let (px, py) = (-(b.1 - a.1) / l * 1.75, (b.0 - a.0) / l * 1.75);
        let id = format!("{var}@L1:branch_line:full_privilege#{n}");
        let side = |s: f64, part: &str| format!(
            "<line data-hash=\"{var}\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" data-rv-id=\"{id}/{part}\" \
             data-rv-kind=\"branch_line\" data-rv-part=\"{part}\" data-rv-var=\"{var}\"/>",
            a.0 + s * px, a.1 + s * py, b.0 + s * px, b.1 + s * py);
        format!("{}{}<polygon data-hash=\"{var}\" points=\"0,0 1,1 2,2\" data-rv-id=\"{id}\" data-rv-kind=\"branch_line\" data-rv-var=\"{var}\"/>",
                side(1.0, "side1"), side(-1.0, "side2"))
    }
    fn arrow(id: &str, var: &str, to: &str, pts: &str, head: &str) -> String {
        format!("<g data-rv-id=\"{id}\" data-rv-kind=\"arrow\" data-rv-var=\"{var}\" data-rv-to=\"{to}\" \
                 data-rv-line=\"1\" data-tooltip-text=\"{id}\"><polyline class=\"arrow-shaft\" points=\"{pts}\"/>\
                 <polygon class=\"arrow-head\" points=\"{head}\"/></g>")
    }
    fn header(var: &str) -> String {
        format!("<text x=\"10\" y=\"10\" data-rv-id=\"hdr:{var}\" data-rv-kind=\"header\" data-rv-var=\"{var}\">{var}</text>")
    }

    /// `let a = if c { f } else if d { g } else { h };` (#1278): an `else`
    /// arm whose column forks again. Drawn right, the outer arm's leg ends
    /// at the inner fork and the inner join's leg goes on to the outer join.
    fn else_if(stub: bool) -> String {
        let mut s = [
            dot("a", 100.0, 100.0, "branch"), dot("a", 150.0, 130.0, "branch"),
            dot("a", 125.0, 160.0, "acquire"), dot("a", 175.0, 160.0, "acquire"),
            dot("a", 150.0, 190.0, "branch_join"), dot("a", 100.0, 220.0, "branch_join"),
            dot("a", 75.0, 160.0, "acquire"),
            arm("a", 1, (100.0, 100.0), (75.0, 130.0)), arm("a", 2, (75.0, 130.0), (75.0, 190.0)),
            arm("a", 3, (75.0, 190.0), (100.0, 220.0)),
            arm("a", 4, (100.0, 100.0), (150.0, 115.0)),
            arm("a", 5, (150.0, 130.0), (125.0, 160.0)), arm("a", 6, (150.0, 130.0), (175.0, 160.0)),
            arm("a", 7, (125.0, 160.0), (150.0, 190.0)), arm("a", 8, (175.0, 160.0), (150.0, 190.0)),
            arm("a", 9, (150.0, 190.0), (100.0, 220.0)),
        ].concat();
        // The outer arm's column, down to the inner fork, or past it.
        s.push_str(&arm("a", 10, (150.0, 115.0), (150.0, if stub { 145.0 } else { 130.0 })));
        s
    }

    #[test]
    fn a_column_drawn_past_a_fork_is_caught() {
        assert_eq!(invs(&else_if(false)), Vec::<&str>::new());
        assert_eq!(invs(&else_if(true)), vec!["line-ends-nowhere", "line-through-fork"]);
    }

    #[test]
    fn a_fork_or_join_with_no_dot_is_caught() {
        // #1371: the legs of a column lent out across an `if let` forked
        // and joined in mid-line.
        for (gone, inv) in [("branch 150 130", "fork-without-dot"), ("branch_join 100 220", "fork-without-dot")] {
            let body = else_if(false).split_inclusive("/>")
                .filter(|el| !el.contains(&format!("data-tooltip-text=\"{gone}\"")))
                .collect::<String>();
            assert_eq!(invs(&body), vec![inv], "{gone}");
        }
    }

    #[test]
    fn columns_start_and_end_at_dots_or_joints() {
        let ok = [dot("s", 70.0, 95.0, "acquire"), line("s", 70.0, 95.0, 125.0), line("s", 70.0, 125.0, 155.0),
                  dot("s", 70.0, 155.0, "owner_go_out_of_scope")].concat();
        assert_eq!(invs(&ok), Vec::<&str>::new());
        // A gap: the column stops on row 125 and starts again on row 140.
        let gap = ok.replace("y1=\"125\"", "y1=\"140\"");
        assert_eq!(invs(&gap), vec!["line-ends-nowhere", "line-starts-nowhere"]);
        // A struct's column stops on the row its field is moved out.
        let partial = [dot("p", 70.0, 95.0, "acquire"), line("p", 70.0, 95.0, 125.0),
                       dot("p.name", 170.0, 125.0, "move")].concat();
        assert_eq!(invs(&partial), Vec::<&str>::new());
        assert_eq!(invs(&partial.replace("p.name", "q")), vec!["line-ends-nowhere"]);
        // A stretch drawn twice (an arm's own row over its column) is fine.
        let twice = [ok.clone(), line("s", 70.0, 100.0, 110.0)].concat();
        assert_eq!(invs(&twice), Vec::<&str>::new());
        // A diverging arm's column ends in a stop bar.
        let stop = [dot("s", 70.0, 95.0, "acquire"), line("s", 70.0, 95.0, 125.0),
                    "<line data-hash=\"s\" x1=\"64\" y1=\"125\" x2=\"76\" y2=\"125\" data-rv-id=\"s@L2:stop_bar\" data-rv-kind=\"stop_bar\"/>".into()].concat();
        assert_eq!(invs(&stop), Vec::<&str>::new());
    }

    #[test]
    fn arrows_along_one_stretch_are_caught() {
        let f = arrow("f>a", "f", "a", "75 135 300 135", "300,129 312.75,135 300,141");
        let h = arrow("h>a", "h", "a", "195 135 300 135", "300,129 312.75,135 300,141");
        let k = arrow("k>b", "k", "b", "75 165 300 165", "300,159 312.75,165 300,171");
        assert_eq!(invs(&[f.clone(), k.clone()].concat()), Vec::<&str>::new());
        // (Their heads lie on one spot too.)
        assert_eq!(invs(&[f.clone(), h].concat()), vec!["arrows-overlap", "heads-or-logos-overlap"]);
        // A one-line function's parameter in and result out touch at the elbow.
        let p = arrow("caller>s", "caller", "s", "98 117 98 125 88 125", "88,131 75.25,125 88,119");
        let r = arrow("s>caller", "s", "caller", "75.25 125 98 125 98 120", "92,120 98,107.25 104,120");
        assert_eq!(invs(&[p, r].concat()), Vec::<&str>::new());
        // #1468: a level return on the track half a row below, and a
        // binding arrow rising across that track at under 5 degrees: their
        // strokes lie on each other for 47 px.
        let ret = arrow("pair>a", "pair", "a", "305.8 218.15 290 230 90 230 84.4 225.8", "80.8,230.6 74.2,218.15 88,221");
        let slant = arrow("a>x", "a", "x", "100.23 244.56 432.06 216.52", "431.56,210.54 444.77,215.44 432.57,222.49");
        assert_eq!(invs(&[ret.clone(), slant].concat()), vec!["arrows-overlap"]);
        // Crossing it at 45 or 30 degrees, it touches the track for 7 or
        // 10 px.
        let steep = arrow("a>x", "a", "x", "150 280 250 180", "245.76,175.76 259.02,170.98 254.24,184.24");
        let shallow = arrow("a>x", "a", "x", "150 260 300 173.4", "297,168.2 311,167 303,178.6");
        assert_eq!(invs(&[ret.clone(), steep].concat()), Vec::<&str>::new());
        assert_eq!(invs(&[ret, shallow].concat()), Vec::<&str>::new());
        // A hop arrow's straight pieces count, its arc doesn't.
        assert_eq!(path_pieces("M 145.25 335 L 300 335 A 10 10 0 0 1 320 335 L 362 335"),
                   vec![((145.25, 335.0), (300.0, 335.0)), ((320.0, 335.0), (362.0, 335.0))]);
    }

    #[test]
    fn heads_and_logos_on_other_arrows_are_caught() {
        // g runs level along row 135; h comes up to it from below.
        let g = arrow("g>a", "g", "a", "75 135 300 135", "300,129 312.75,135 300,141");
        let clear = arrow("h>b", "h", "b", "200 200 200 160", "194,160 200,147.25 206,160");
        assert_eq!(invs(&[g.clone(), clear].concat()), Vec::<&str>::new());
        // h's head ends on g's shaft: under `OVERLAP_PX` of shared stretch.
        let onto = arrow("h>b", "h", "b", "200 200 200 141", "194,141 200,128.25 206,141");
        assert_eq!(invs(&[g.clone(), onto].concat()), vec!["heads-or-logos-overlap"]);
        // A shaft passing by g's head, its centre line 3 px off the triangle
        // (its edge 0.5 px off): touching (#1316's `y -> a` at
        // `chain_block_if_1287` line 54).
        let by = arrow("k>c", "k", "c", "100 126 400 126", "400,120 412.75,126 400,132");
        assert_eq!(invs(&[g.clone(), by.clone()].concat()), vec!["heads-or-logos-overlap"]);
        assert_eq!(invs(&[g.clone(), by.replace("126", "125")].concat()), Vec::<&str>::new());
        // An arrow leaving a's column may come that close to a head into it.
        let from_a = arrow("a>c", "a", "c", "100 126 400 126", "400,120 412.75,126 400,132");
        assert_eq!(invs(&[g.clone(), from_a].concat()), Vec::<&str>::new());
        // So may one hopping over the dot the head points at: a head coming
        // up into b's dot under the hop (`match_copy_ref_out_of_tuple` l6).
        let hop = "<g data-rv-id=\"p>a\" data-rv-kind=\"arrow\" data-rv-var=\"p\" data-rv-to=\"a\" data-rv-line=\"1\" \
                   data-tooltip-text=\"p>a\"><path class=\"arrow-shaft\" d=\"M 295.75 215 L 150 215 A 10 10 0 0 0 130 215 L 88 215\"/>\
                   <polygon class=\"arrow-head\" points=\"88,209 75.25,215 88,221\"/></g>";
        let up = arrow("q>b", "q", "b", "155.38 224.35 160 240", "152.26,229.48 144.49,217.73 158.5,219.23");
        assert_eq!(invs(&[hop.to_string(), up.clone()].concat()), Vec::<&str>::new());
        assert_eq!(invs(&[hop.replace("A 10 10 0 0 0 130 215", "L 130 215"), up].concat()), vec!["heads-or-logos-overlap"]);
        // A call's logo on an arrow not its own.
        let logo = "<text x=\"150\" y=\"140\" data-rv-id=\"q>caller@L5:fn:move\" data-rv-kind=\"fn\" \
                    data-rv-var=\"q\" data-rv-to=\"caller\" data-tooltip-text=\"q\">f</text>".to_string();
        assert_eq!(invs(&[g, logo].concat()), vec!["heads-or-logos-overlap"]);
    }

    #[test]
    fn shafts_through_dots_and_stop_bars_are_caught() {
        // `match (a, (b, c))` (#1344): b's move to x dips half a row, onto
        // the row of c's dot.
        let c = dot("c", 210.0, 245.0, "move");
        let through = arrow("b>x", "b", "x", "142.9 219.4 160 245 330 245 340 230", "334.8,226.7 345.6,219.3 344.8,233.3");
        assert_eq!(invs(&[c.clone(), through].concat()), vec!["shaft-through-mark"]);
        // On a track that misses it, or hopping it, it's fine.
        let clear = arrow("b>x", "b", "x", "144.2 218.2 160 230 330 230 335.6 225.8", "332,221 345.8,218 339.2,230.6");
        assert_eq!(invs(&[c.clone(), clear].concat()), Vec::<&str>::new());
        let hop = "<g data-rv-id=\"b>x\" data-rv-kind=\"arrow\" data-rv-var=\"b\" data-rv-to=\"x\" data-rv-line=\"1\" \
                   data-tooltip-text=\"b>x\"><path class=\"arrow-shaft\" d=\"M 142.9 219.4 L 160 245 L 200 245 A 10 10 0 0 1 220 245 L 330 245 L 340 230\"/>\
                   <polygon class=\"arrow-head\" points=\"334.8,226.7 345.6,219.3 344.8,233.3\"/></g>";
        assert_eq!(invs(&[c.clone(), hop.to_string()].concat()), Vec::<&str>::new());
        // Its own dots (5.25 px off their centres) don't count.
        let own = [dot("b", 70.0, 245.0, "move"), dot("x", 300.0, 245.0, "acquire"),
                   arrow("b>x", "b", "x", "75.25 245 282 245", "282,239 294.75,245 282,251")].concat();
        assert_eq!(invs(&own), Vec::<&str>::new());
        // A return along the row of a `break` arm's stop bars
        // (`loop_break_call_arg`): they look like legs ending on the shaft.
        let bar = "<line data-hash=\"f\" x1=\"184\" y1=\"175\" x2=\"196\" y2=\"175\" data-rv-id=\"f@L31:stop_bar\" data-rv-kind=\"stop_bar\"/>";
        let along = arrow("re>e", "re", "e", "374.75 175 138 175", "138,169 125.25,175 138,181");
        assert_eq!(invs(&[bar, &along].concat()), vec!["shaft-through-mark"]);
        assert_eq!(invs(&[bar, &along.replace("175 138 175", "190 138 190")].concat()), Vec::<&str>::new());
    }

    #[test]
    fn arrows_end_to_end_are_caught() {
        // `r.name = a` then r's borrow returned to p on one row (#1349):
        // the move's head at r's dot, the return leaving it straight on.
        let cols = [header("a"), header("r"), header("p"), dot("a", 420.0, 335.0, "move"),
                    dot("r", 300.0, 335.0, "mutable_die"), dot("p", 40.0, 335.0, "mutable_reacquire")].concat();
        let a_r = arrow("a>r", "a", "r", "414.75 335 318 335", "318,329 305.25,335 318,341");
        let r_p = arrow("r>p", "r", "p", "294.75 335 58 335", "58,329 45.25,335 58,341");
        assert_eq!(invs(&[cols.clone(), a_r.clone(), r_p].concat()), vec!["arrows-end-to-end"]);
        // The return dipping half a row instead.
        let dips = arrow("r>p", "r", "p", "295.8 338.15 280 350 60 350 54.4 345.8", "50.8,350.6 44.2,339 57.9,341");
        assert_eq!(invs(&[cols.clone(), a_r.clone(), dips].concat()), Vec::<&str>::new());
        // Two heads meeting at a dot from either side read as two arrows.
        let p_r = arrow("p>r", "p", "r", "45.25 335 282 335", "282,329 294.75,335 282,341");
        assert_eq!(invs(&[cols.clone(), a_r.clone(), p_r].concat()), Vec::<&str>::new());
        // Nor a call's short arrow on to its logo (`opt>s` then `s>show`).
        let call = arrow("r>show", "r", "show", "294.75 335 270 335", "270,329 257.25,335 270,341");
        assert_eq!(invs(&[cols, a_r, call].concat()), Vec::<&str>::new());
    }

    #[test]
    fn hidden_marks_and_stray_arrows_are_caught() {
        let two = [dot("p", 70.0, 95.0, "acquire"), dot("p", 70.0, 95.0, "mutable_reacquire")].concat();
        assert_eq!(invs(&two), vec!["marks-on-one-spot"]);
        let logo = |f: &str| format!("<text x=\"14\" y=\"190\" data-rv-id=\"{f}>caller@L5:fn:move\" data-rv-kind=\"fn\" \
                                      data-rv-var=\"{f}\" data-rv-to=\"caller\" data-tooltip-text=\"{f}\">f</text>");
        assert_eq!(invs(&[logo("make"), logo("make")].concat()), Vec::<&str>::new());
        assert_eq!(invs(&[logo("make"), logo("String::from")].concat()), vec!["heads-or-logos-overlap", "marks-on-one-spot"]);
        let to_a = [header("a"), dot("a", 70.0, 95.0, "acquire")].concat();
        let hit = arrow("f>a", "f", "a", "200 95 88 95", "88,101 75.25,95 88,89");
        assert_eq!(invs(&[to_a.clone(), hit].concat()), Vec::<&str>::new());
        let miss = arrow("f>a", "f", "a", "200 125 88 125", "88,131 75.25,125 88,119");
        assert_eq!(invs(&[to_a, miss].concat()), vec!["arrow-misses-target"]);
        assert_eq!(invs(&dot("s", 70.0, 520.0, "acquire")), vec!["outside-figure"]);
    }
}

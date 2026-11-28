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
    "fork-without-dot", "arrow-along-leg", "arrow-near-dot", "arrow-in-gutter",
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
/// An arrow's visible ends stop this close to the event dots' centres
/// (8.2 px for a diagonal head).
const ARROW_REACH: f64 = 10.0;
/// At the first column allow a dot radius and half the width of its arrow shaft.
const GUTTER_MARGIN: f64 = 8.0;
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
    arm_rows: Option<String>,
    state: String,
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
    line: String,
    radius: f64,
    painted_radius: f64,
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
    event: String,
    /// The shaft's straight pieces (a hop's arc left out).
    pieces: Vec<((f64, f64), (f64, f64))>,
    hops: Vec<Hop>,
    miters: Vec<(Pt, Pt)>,
    head: Option<(f64, f64)>,
    /// The head's triangle.
    tri: Vec<(f64, f64)>,
}

impl Arrow {
    /// Shaft geometry and head vertices. A horizontal half-hop reaches its
    /// vertical extreme at the midpoint. The parser flattens other SVG arcs.
    fn path_points(&self) -> impl Iterator<Item = Pt> + '_ {
        self.pieces.iter().flat_map(|&(p, q)| [p, q])
            .chain(self.tri.iter().copied())
            .chain(self.hops.iter().flat_map(|h| {
                [h.chord.0, h.chord.1, h.point_at(h.start_angle + h.sweep / 2.0)].into_iter()
                    .chain(h.flattened.iter().map(|&(_, q)| q))
            }))
    }

    /// The shaft uses butt caps and miter joins. The head is a filled triangle.
    fn painted_points(&self) -> impl Iterator<Item = Pt> + '_ {
        self.pieces.iter().filter_map(|&(p, q)| {
            let length = len(p, q);
            if length == 0.0 { return None }
            let nx = SHAFT_HALF * (p.1 - q.1) / length;
            let ny = SHAFT_HALF * (q.0 - p.0) / length;
            Some([(p.0 + nx, p.1 + ny), (p.0 - nx, p.1 - ny),
                  (q.0 + nx, q.1 + ny), (q.0 - nx, q.1 - ny)])
        }).flatten().chain(self.hops.iter().flat_map(Hop::stroke_extrema))
            .chain(self.miters.iter().flat_map(|&(p, q)| [p, q]))
            .chain(self.tri.iter().copied())
    }

}

/// An SVG elliptical arc in an arrow shaft, in centre parameterization.
#[derive(Debug, Clone)]
struct Hop {
    chord: (Pt, Pt),
    center: Pt,
    radii: Pt,
    axis: Pt,
    start_angle: f64,
    sweep: f64,
    /// Other than the renderer's circular half-hops, approximate the arc
    /// with segments short enough to have <0.001 px chord error.
    flattened: Vec<(Pt, Pt)>,
}

impl Hop {
    fn from_svg(chord @ (from, to): (Pt, Pt), radii: Pt, rotation: f64, large: bool, forwards: bool) -> Option<Self> {
        let (mut rx, mut ry) = (radii.0.abs(), radii.1.abs());
        if from == to || rx == 0.0 || ry == 0.0 {
            return None;
        }
        // SVG 2, B.2.4–5: rotate to the ellipse's axes and scale
        // radii that are too small to connect the endpoints.
        let (sin, cos) = rotation.to_radians().sin_cos();
        let (dx, dy) = ((from.0 - to.0) / 2.0, (from.1 - to.1) / 2.0);
        let (x, y) = (cos * dx + sin * dy, -sin * dx + cos * dy);
        let scale = (x / rx).hypot(y / ry);
        rx *= scale.max(1.0);
        ry *= scale.max(1.0);
        let ratio = (x / rx).powi(2) + (y / ry).powi(2);
        let side = if large == forwards { -1.0 } else { 1.0 };
        let offset = side * ((1.0 - ratio).max(0.0) / ratio).sqrt();
        let (cx, cy) = (offset * rx * y / ry, -offset * ry * x / rx);
        let midpoint = mid(from, to);
        let center = (midpoint.0 + cos * cx - sin * cy, midpoint.1 + sin * cx + cos * cy);
        let start_angle = ((y - cy) / ry).atan2((x - cx) / rx);
        let end_angle = ((-y - cy) / ry).atan2((-x - cx) / rx);
        let tau = std::f64::consts::TAU;
        let sweep = if forwards { (end_angle - start_angle).rem_euclid(tau) }
            else { -(start_angle - end_angle).rem_euclid(tau) };
        let mut arc = Self { chord, center, radii: (rx, ry), axis: (cos, sin), start_angle, sweep, flattened: Vec::new() };
        if !arc.is_horizontal_semicircle() {
            let max_radius = rx.max(ry);
            let count = (sweep.abs() * (max_radius / 0.008).sqrt()).ceil().max(1.0) as usize;
            let mut p = from;
            for k in 1..=count {
                let q = if k == count { to } else { arc.point_at(start_angle + sweep * k as f64 / count as f64) };
                arc.flattened.push((p, q));
                p = q;
            }
        }
        Some(arc)
    }

    fn is_horizontal_semicircle(&self) -> bool {
        self.radii.0 == self.radii.1 && self.chord.0.1 == self.chord.1.1
            && len(self.center, mid(self.chord.0, self.chord.1)) < 1e-9
    }

    fn rotate(&self, (x, y): Pt) -> Pt {
        let (cos, sin) = self.axis;
        (cos * x - sin * y, sin * x + cos * y)
    }

    fn point_at(&self, angle: f64) -> Pt {
        let (sin, cos) = angle.sin_cos();
        let (x, y) = self.rotate((self.radii.0 * cos, self.radii.1 * sin));
        (self.center.0 + x, self.center.1 + y)
    }

    /// Stroke edges at the caps and the x/y extrema swept by this arc.
    fn stroke_extrema(&self) -> impl Iterator<Item = Pt> + '_ {
        let (rx, ry) = self.radii;
        let (cos, sin) = self.axis;
        let x_extreme = (-ry * sin).atan2(rx * cos);
        let y_extreme = (ry * cos).atan2(rx * sin);
        let pi = std::f64::consts::PI;
        let extrema = [x_extreme, x_extreme + pi, y_extreme, y_extreme + pi];
        [self.start_angle, self.start_angle + self.sweep].into_iter()
            .chain(extrema.into_iter().filter(move |angle| {
                ((angle - self.start_angle) * self.sweep.signum()).rem_euclid(2.0 * pi) <= self.sweep.abs()
            }))
            .flat_map(move |angle| {
                let (x, y) = self.point_at(angle);
                let (sin, cos) = angle.sin_cos();
                let (nx, ny) = self.rotate((ry * cos, rx * sin));
                let scale = SHAFT_HALF / nx.hypot(ny);
                [(x + scale * nx, y + scale * ny), (x - scale * nx, y - scale * ny)]
            })
    }

    fn tangent(&self, at_end: bool) -> Pt {
        let angle = self.start_angle + if at_end { self.sweep } else { 0.0 };
        let (sin, cos) = angle.sin_cos();
        let (x, y) = self.rotate((-self.radii.0 * sin, self.radii.1 * cos));
        let scale = self.sweep.signum() / x.hypot(y);
        (x * scale, y * scale)
    }

    fn distance_to(&self, point: Pt) -> f64 {
        let (from, to) = self.chord;
        if self.flattened.is_empty() {
            let above = (to.0 > from.0) == (self.sweep > 0.0);
            if (above && point.1 <= from.1) || (!above && point.1 >= from.1) {
                (len(point, self.center) - self.radii.0).abs()
            } else {
                len(point, from).min(len(point, to))
            }
        } else {
            self.flattened.iter().map(|&(p, q)| off_piece(point, p, q)).fold(f64::INFINITY, f64::min)
        }
    }
}

/// A function logo (`fn`).
#[derive(Debug, Clone)]
struct Logo {
    id: String,
    event: String,
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

/// A hop arrow's straight pieces and arcs.
fn path_parts(d: &str) -> (Vec<(Pt, Pt)>, Vec<Hop>) {
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
                "A" => {
                    let Some((rx, ry)) = f(i + 1).zip(f(i + 2)) else { break };
                    let Some(rotation) = f(i + 3) else { break };
                    if let Some(arc) = Hop::from_svg((from, to), (rx, ry), rotation, f(i + 4) == Some(1.0), f(i + 5) == Some(1.0)) {
                        hops.push(arc);
                    } else if from != to {
                        out.push((from, to));
                    }
                }
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
                let radius = num(c, "r");
                g.dots.push(Dot {
                    id, var: a("data-rv-var"), hash, x: num(c, "cx"), y: num(c, "cy"), line: a("data-rv-line"), radius,
                    painted_radius: radius + c.attribute("stroke-width")
                        .and_then(|width| width.trim_end_matches("px").parse::<f64>().ok()).unwrap_or(1.0) / 2.0,
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
                g.segs.push(Seg {
                    id, var: a("data-rv-var"), hash, main: true, arm_rows: None,
                    state: a("data-rv-state"), a: p, b: q,
                });
            }
            "branch_line" => {
                let Some(part @ ("line" | "side1" | "side2")) = n.attribute("data-rv-part") else { continue };
                let order = arms.len();
                let e = arms.entry(base.clone()).or_insert_with(|| (order, Seg {
                    id: base, var: a("data-rv-var"), hash, main: false,
                    arm_rows: n.parent().and_then(|g| g.attribute("data-arm-rows")).map(str::to_owned),
                    state: a("data-rv-state"), a: (0.0, 0.0), b: (0.0, 0.0),
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
                let mut miters = Vec::new();
                for c in n.descendants().filter(|c| c.attribute("class").is_some_and(|k| k.split(' ').any(|k| k == "arrow-shaft"))) {
                    let (straight, arcs) = if c.has_tag_name("polyline") {
                        let p = points(c.attribute("points").unwrap_or(""));
                        (p.windows(2).map(|w| (w[0], w[1])).collect(), Vec::new())
                    } else {
                        path_parts(c.attribute("d").unwrap_or(""))
                    };
                    miters.extend(shaft_miters(&straight, &arcs));
                    pieces.extend(straight);
                    hops.extend(arcs);
                }
                let tri = n.descendants()
                    .find(|c| c.attribute("class").is_some_and(|k| k.split(' ').any(|k| k == "arrow-head")))
                    .map(|c| points(c.attribute("points").unwrap_or(""))).unwrap_or_default();
                let head = tri.get(1).copied();
                g.arrows.push(Arrow {
                    id, var: a("data-rv-var"), to: a("data-rv-to"), line: a("data-rv-line"),
                    event: a("data-rv-event"), tip: a("data-tooltip-text"), pieces, hops, miters, head, tri,
                });
            }
            "fn" => g.logos.push(Logo {
                event: a("data-rv-event"), id, var: a("data-rv-var"), to: a("data-rv-to"), line: a("data-rv-line"),
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

impl Geo {
    /// Centre-line endpoints and code-panel arm rows, in drawing order.
    pub fn branch_segments<'a>(&'a self, var: &'a str) -> impl Iterator<Item = (Pt, Pt, Option<&'a str>)> + 'a {
        self.segs.iter().filter(move |s| !s.main && s.var == var)
            .map(|s| (s.a, s.b, s.arm_rows.as_deref()))
    }

    /// Centre-line endpoints of main column segments in the given state.
    pub fn main_segments<'a>(&'a self, var: &'a str, state: &'a str) -> impl Iterator<Item = (Pt, Pt)> + 'a {
        self.segs.iter().filter(move |s| s.main && s.var == var && s.state == state)
            .map(|s| (s.a, s.b))
    }

    /// Check that the shaft starts at the source dot and the head reaches
    /// the target dot. Both are set back a few pixels from the dot centres.
    pub fn arrow_connects(
        &self,
        var: &str,
        to: &str,
        line: &str,
        (from, target): (Pt, Pt),
        matches_tip: impl Fn(&str) -> bool,
    ) -> bool {
        self.arrows.iter().any(|a| {
            a.var == var && a.to == to && a.line == line && matches_tip(&a.tip)
                && a.pieces.first().is_some_and(|&(start, _)| len(start, from) <= ARROW_REACH)
                && a.head.is_some_and(|head| len(head, target) <= ARROW_REACH)
        })
    }

    fn endpoint_dots(&self, a: &Arrow) -> Option<(&Dot, &Dot)> {
        let dot = |var: &str, point| self.dots.iter()
            .find(|d| d.var == var && d.line == a.line && len((d.x, d.y), point) <= ARROW_REACH);
        dot(&a.var, a.pieces.first()?.0).zip(dot(&a.to, a.head?))
    }

    /// Straight shaft pieces and hop chords must stay on the event row.
    pub fn arrow_stays_on_row(&self, var: &str, to: &str, line: &str, event: &str, crossed: &[&str]) -> bool {
        self.arrows.iter().any(|a| {
            if a.var != var || a.to != to || a.line != line || a.event != event {
                return false;
            }
            let Some((source, target)) = self.endpoint_dots(a) else { return false };
            let (left, right) = (source.x.min(target.x), source.x.max(target.x));
            let row = source.y;
            row == target.y && a.head.is_some_and(|h| (h.1 - row).abs() < 1e-8)
                && a.pieces.iter().all(|(p, q)| (p.1 - row).abs() < 1e-8 && (q.1 - row).abs() < 1e-8)
                && a.hops.iter().all(|h| h.is_horizontal_semicircle() && h.chord.0.1 == row)
                && a.path_points().all(|(x, y)| (left - 0.01..=right + 0.01).contains(&x) && (y - row).abs() <= 15.0)
                && crossed.iter().all(|col| a.hops.iter().any(|hop| {
                    let center = mid(hop.chord.0, hop.chord.1);
                    self.dots.iter().any(|d| d.var == *col && d.line == line && near((d.x, d.y), center))
                        || self.segs.iter().any(|s| !s.main && s.var == *col
                            && off_piece(center, s.a, s.b) <= TOL)
                }))
        })
    }

    /// On a displaced track, a hop leaves a gap in the straight shaft where
    /// the branch leg crosses. Derive the intersection from the drawn leg.
    pub fn return_clears_off_row_legs(&self, var: &str, to: &str, line: &str, branch: &str) -> bool {
        self.arrows.iter().any(|a| {
            if a.var != var || a.to != to || a.line != line || a.event != "static_die" { return false }
            let Some((source, target)) = self.endpoint_dots(a) else { return false };
            if source.y != target.y { return false }
            let mut legs = self.segs.iter().filter(|s| !s.main && s.var == branch).peekable();
            legs.peek().is_some() && legs.all(|s| a.pieces.iter().all(|&(p, q)| {
                if p.1 == source.y || p.1 != q.1 || s.a.1 == s.b.1 { return true }
                let t = (p.1 - s.a.1) / (s.b.1 - s.a.1);
                let x = s.a.0 + t * (s.b.0 - s.a.0);
                !(0.0..=1.0).contains(&t) || x < p.0.min(q.0) || x > p.0.max(q.0)
            }))
        })
    }

    pub fn has_leftward_borrow_end_stub(&self, var: &str, line: &str) -> bool {
        self.arrows.iter().any(|a| {
            let ([(start, base)], Some(head)) = (a.pieces.as_slice(), a.head) else { return false };
            a.var == var && a.to == var && a.line == line && a.event == "static_die" && a.hops.is_empty()
                && head.0 < base.0 && base.0 < start.0 && start.1 == base.1 && base.1 == head.1
                && self.dots.iter().any(|d| d.var == var && d.line == line && d.y == start.1
                    && d.x > start.0 && d.x - start.0 <= ARROW_REACH && d.x - head.0 <= 30.75)
        })
    }

    /// Keep every route between the columns it connects, with room for a
    /// short head approach from the far side.
    pub fn arrows_stay_between_columns(&self, from: &str, to: &str, line: &str, jog: f64) -> bool {
        let mut arrows = self.arrows.iter().filter(|a| a.var == from && a.to == to && a.line == line).peekable();
        arrows.peek().is_some() && arrows.all(|a| {
            let Some((source, target)) = self.endpoint_dots(a) else { return false };
            let span = (source.x.min(target.x) - jog)..=(source.x.max(target.x) + jog);
            a.pieces.iter().flat_map(|&(p, q)| [p, q])
                .chain(a.tri.iter().copied())
                .chain(a.hops.iter().flat_map(|hop| [hop.chord.0, hop.chord.1]))
                .all(|(x, _)| span.contains(&x))
        })
    }
}

pub fn near(p: (f64, f64), q: (f64, f64)) -> bool {
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

/// Outer edges of the painted joins (SVG defaults to miter, limit 4).
fn shaft_miters(pieces: &[(Pt, Pt)], arcs: &[Hop]) -> Vec<(Pt, Pt)> {
    let mut edges = Vec::new();
    let mut join = |at: Pt, incoming: Pt, outgoing: Pt| {
        let (dx, dy) = incoming;
        let (ex, ey) = outgoing;
        let cross = dx * ey - dy * ex;
        if cross.abs() < 1e-9 {
            return;
        }
        let scale = SHAFT_HALF * cross.signum();
        let (nx, ny) = (scale * dy, -scale * dx);
        let (mx, my) = (scale * ey, -scale * ex);
        let alignment = 1.0 + dx * ex + dy * ey;
        let tip = ((nx + mx) / alignment, (ny + my) / alignment);
        if tip.0.hypot(tip.1) <= 4.0 * SHAFT_HALF {
            let (x, y) = at;
            let corner = (x + tip.0, y + tip.1);
            edges.extend([((x + nx, y + ny), corner), (corner, (x + mx, y + my))]);
        }
    };
    let mut lines = pieces.iter().copied().filter_map(|(p, q)| {
        let length = len(p, q);
        (length > 1e-9).then(|| ((p, q), ((q.0 - p.0) / length, (q.1 - p.1) / length)))
    }).peekable();
    while let Some(((p, q), tangent)) = lines.next() {
        if let Some(&(_, next)) = lines.peek().filter(|((start, _), _)| *start == q) {
            join(q, tangent, next);
        }
        for arc in arcs {
            if q == arc.chord.0 {
                join(q, tangent, arc.tangent(false));
            }
            if p == arc.chord.1 {
                join(p, arc.tangent(true), tangent);
            }
        }
    }
    edges
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

    // Two marks with one tooltip are one arrow (an arrow and its call's
    // logo, a call's results to the caller), except two arrows between
    // columns on different lines: the same return drawn on two rows of an
    // arm's lines, along one track, reads as one arrow with two heads
    // (#1559).
    let on_columns = |a: &Arrow| a.var != a.to && g.headers.contains(&a.var) && g.headers.contains(&a.to);
    let one_arrow = |a: &Arrow, b: &Arrow| a.tip == b.tip && (a.line == b.line || !on_columns(a) || !on_columns(b));

    // Two arrows along one stretch: the pointer reaches only the one drawn
    // on top (#1264, #1187). On one track, or one slanting across the
    // other's track at a shallow angle, their centre lines within half a
    // stroke of each other (#1468: a level return and a binding arrow
    // rising 28 px over 330 px lay on each other for 47 px; two returns
    // leaving one dot 6 degrees apart, for 19 px).
    for (i, a) in g.arrows.iter().enumerate() {
        for b in &g.arrows[i + 1..] {
            if one_arrow(a, b) {
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
        if !g.dots.iter().any(|d| d.var == a.to && len(h, (d.x, d.y)) <= ARROW_REACH) {
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
            if std::ptr::eq(a, b) || one_arrow(a, b) || a.tri.len() < 3 || (elbow && a.line == b.line) {
                continue;
            }
            // b's shaft (2.5 px either side of its centre line) or head in a's head.
            // A shaft that starts or ends at the head's column, or hops over
            // the dot the head points at, can't keep clear of it there: it
            // may come up to the head. One passing by may not touch it (its
            // edge within 1 px of the triangle, #1316's `y -> a` grazing
            // `a -> z`'s head).
            let related = b.var == a.to || b.to == a.to
                || a.head.is_some_and(|h| b.hops.iter().any(|hop| len(mid(hop.chord.0, hop.chord.1), h) < 8.0));
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
    // see below. A head also lying along a column's vertical stretch
    // (#1488: a copy dipping two rows came back up into its target's
    // first dot at about 70 degrees, its head over the column under the
    // dot); a shaft there is other checks' business.
    for a in &g.arrows {
        for s in &g.segs {
            let covered = if a.tri.len() == 3 {
                samples(&[(s.a, s.b)]).into_iter().filter(|&p| inside(&a.tri, p) > -1.0).count()
            } else {
                0
            };
            // A column's vertical stretch: its head only (#1488).
            if (s.a.0 - s.b.0).abs() <= TOL {
                if covered as f64 > HEAD_ON_LEG_PX {
                    out.push(("arrow-along-leg", format!("head of {} lies along {} for {} px", a.id, s.id, covered)));
                }
                continue;
            }
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

    // A shaft through a dot's centre (#1344) makes it read as part of the
    // arrow and takes its hover. Even when the shaft misses the centre,
    // an unrelated dot needs a visible gap from the painted arrow.
    for a in &g.arrows {
        let head_extent = a.head.map_or(0.0, |h| a.tri.iter().map(|&corner| len(h, corner)).fold(0.0, f64::max));
        for d in &g.dots {
            let center = (d.x, d.y);
            let clearance = d.painted_radius + 1.0;
            let shaft_gap = a.pieces.iter().enumerate().filter_map(|(i, &(p, q))| {
                let distance = off_piece(center, p, q);
                // The shaft may leave its source dot at the rim. A bend
                // or another part of the shaft still needs clearance.
                let leaves_dot = i == 0 && a.var == d.var && len(p, center) <= ARROW_REACH
                    && distance >= d.radius - 0.01 && distance >= len(p, center) - 0.01;
                (!leaves_dot).then_some(distance)
            }).fold(f64::INFINITY, f64::min);
            if shaft_gap < SHAFT_HALF {
                out.push(("shaft-through-mark", format!("{} runs through {} at {}", a.id, d.id, fmt(center))));
                continue;
            }
            if shaft_gap < clearance + SHAFT_HALF
                || a.hops.iter().any(|hop| hop.distance_to(center) < clearance + SHAFT_HALF)
                || a.miters.iter().any(|&(p, q)| off_piece(center, p, q) < clearance)
                || a.head.is_some_and(|h| len(h, center) < head_extent + clearance && off_tri(&a.tri, center) < clearance
                    && !(a.to == d.var && len(h, center) <= ARROW_REACH))
            {
                out.push(("arrow-near-dot", format!("{} passes too close to {} at {}", a.id, d.id, fmt(center))));
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
    let first_column = g.dots.iter().map(|d| d.x)
        .chain(g.segs.iter().flat_map(|s| [s.a.0, s.b.0])).reduce(f64::min);
    for a in &g.arrows {
        // A caller or function logo can be an endpoint in the gutter.
        // Include the endpoints, but never let an intermediate bend define the limit.
        if let (Some(left), Some(&(start, _)), Some(head)) = (first_column, a.pieces.first(), a.head) {
            // Only the endpoint beside the matching glyph gets its clearance.
            // A source function can have a header of its own elsewhere.
            let mut source_in_gutter = a.var == "caller" && a.event == "init_ref_param";
            let mut target_in_gutter = a.to == "caller" && matches!(a.event.as_str(), "move" | "copy");
            for logo in g.logos.iter().filter(|l| l.var == a.var && l.to == a.to && l.line == a.line && l.event == a.event) {
                let center = (logo.x + 5.0, logo.y - 5.0);
                if len(start, center) < len(head, center) {
                    source_in_gutter |= in_box(logo_box(logo), start, 6.0);
                } else {
                    target_in_gutter |= in_box(logo_box(logo), head, 6.0);
                }
            }
            // A local borrow-end stub has its own narrow strip beside the dot.
            let stub = a.var == a.to && matches!(a.event.as_str(), "static_die" | "mutable_die")
                && matches!(a.pieces.as_slice(), [(p, q)] if p.1 == q.1 && q.1 == head.1) && a.hops.is_empty()
                && g.dots.iter().any(|d| d.var == a.var && d.line == a.line && len((d.x, d.y), start) <= ARROW_REACH);
            let edge = left - if stub { 30.75 } else { GUTTER_MARGIN };
            let source_limit = if source_in_gutter { edge.min(start.0 - GUTTER_MARGIN) } else { edge };
            let target_limit = if target_in_gutter { edge.min(head.0 - GUTTER_MARGIN) } else { edge };
            let limit = source_limit.min(target_limit);
            let outside = [(start, source_limit), (head, target_limit)].into_iter()
                .chain(a.path_points().map(|p| (p, limit))).find(|(p, bound)| p.0 < *bound);
            if let Some((p, bound)) = outside {
                out.push(("arrow-in-gutter", format!("{} at {} runs left of {bound:.1}", a.id, fmt(p))));
            }
        }
        if let Some(p) = a.painted_points().find(|&(x, y)| x < -1e-8 || x > w + 1e-8 || y < -1e-8 || y > ht + 1e-8) {
            out.push(("outside-figure", format!("{} at {} outside {w} × {ht}", a.id, fmt(p))));
        }
    }
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
    fn one_return_on_two_lines_along_one_track_is_caught() {
        // #1559: the same return (one tooltip) drawn on two lines of an
        // arm, both along the track between the rows, reads as one arrow
        // with two heads. On one line, one tooltip is one arrow.
        let ret = |line: u32, y: f64| format!(
            "<g data-rv-id=\"r>s@L{line}\" data-rv-kind=\"arrow\" data-rv-var=\"r\" data-rv-to=\"s\" \
             data-rv-line=\"{line}\" data-tooltip-text=\"Return from r to s\"><polyline class=\"arrow-shaft\" \
             points=\"400 {y} 390 137 120 137 110 {y}\"/><polygon class=\"arrow-head\" points=\"0,0 1,1 2,2\"/></g>");
        let cols = [header("r"), header("s")].concat();
        // (The heads are stubs: only the shafts matter here.)
        assert!(invs(&[cols.clone(), ret(1, 125.0), ret(2, 150.0)].concat()).contains(&"arrows-overlap"));
        let same_line = ret(1, 150.0).replace("r>s@L1", "r>s@L1#2");
        assert!(!invs(&[cols, ret(1, 125.0), same_line].concat()).contains(&"arrows-overlap"));
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
        let source = dot("b", 140.0, 185.0, "acquire");
        let c = dot("c", 210.0, 245.0, "move");
        let through = arrow("b>x", "b", "x", "142.9 219.4 160 245 330 245 340 230", "334.8,226.7 345.6,219.3 344.8,233.3");
        assert_eq!(invs(&[source.clone(), c.clone(), through].concat()), vec!["shaft-through-mark"]);
        // On a track that misses it, or hopping it, it's fine.
        let clear = arrow("b>x", "b", "x", "144.2 218.2 160 230 330 230 335.6 225.8", "332,221 345.8,218 339.2,230.6");
        assert_eq!(invs(&[source.clone(), c.clone(), clear].concat()), Vec::<&str>::new());
        let grazing = arrow("b>x", "b", "x", "144 218 160 237 330 237 340 230", "334.8,226.7 345.6,219.3 344.8,233.3");
        assert_eq!(invs(&[source.clone(), c.clone(), grazing].concat()), vec!["arrow-near-dot"]);
        let near_head = dot("q", 349.0, 215.0, "branch_join");
        let head = arrow("b>x", "b", "x", "140 220 330 220", "330,214 342.75,220 330,226");
        assert_eq!(invs(&[source.clone(), near_head, head.clone()].concat()), Vec::<&str>::new());
        let near_head = dot("q", 347.0, 220.0, "branch_join");
        assert_eq!(invs(&[source.clone(), near_head, head].concat()), vec!["arrow-near-dot"]);
        let hop = "<g data-rv-id=\"b>x\" data-rv-kind=\"arrow\" data-rv-var=\"b\" data-rv-to=\"x\" data-rv-line=\"1\" \
                   data-tooltip-text=\"b>x\"><path class=\"arrow-shaft\" d=\"M 142.9 219.4 L 160 245 L 200 245 A 10 10 0 0 1 220 245 L 330 245 L 340 230\"/>\
                   <polygon class=\"arrow-head\" points=\"334.8,226.7 345.6,219.3 344.8,233.3\"/></g>";
        assert_eq!(invs(&[source.clone(), c.clone(), hop.to_string()].concat()), Vec::<&str>::new());
        let near_hop = dot("q", 210.0, 230.0, "branch_join");
        assert_eq!(invs(&[&source, hop, &near_hop].concat()), vec!["arrow-near-dot"]);
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
    fn clearance_includes_the_dot_stroke() {
        let source = dot("b", 9.75, 20.0, "move");
        let near = dot("q", 50.0, 29.0, "branch_join");
        let straight = arrow("b>x", "b", "x", "15 20.25 80 20.25", "80,14.25 92.75,20.25 80,26.25");
        assert_eq!(invs(&format!("{source}{near}{straight}")), vec!["arrow-near-dot"]);
        assert!(invs(&format!("{source}{}{straight}", dot("q", 50.0, 29.25, "branch_join"))).is_empty());
    }

    #[test]
    fn clearance_includes_the_stroke_miter() {
        let source = dot("b", 9.75, 20.0, "move");
        let near = dot("q", 46.0, 13.0, "branch_join");
        let bent = arrow("b>x", "b", "x", "15 20 40 20 40 40", "34,40 40,52.75 46,40");
        assert_eq!(invs(&format!("{source}{near}{bent}")), vec!["arrow-near-dot"]);
        assert!(invs(&format!("{source}{}{bent}", dot("q", 48.0, 13.0, "branch_join"))).is_empty());
    }

    #[test]
    fn crossing_clearance_uses_both_svg_arc_radii() {
        let near = dot("q", 50.0, 50.0, "branch_join");
        let svg = r#"<g data-rv-id="b>x" data-rv-kind="arrow" data-rv-var="b" data-rv-to="x">
            <path class="arrow-shaft" d="M 15 50 L 40 50 A 10 10 0 0 1 60 50 L 90 50"/></g>"#;
        assert!(invs(&[&near, svg].concat()).is_empty());
        for ellipse in ["A 10 3", "A 20 10"] {
            assert_eq!(invs(&format!("{near}{}", svg.replace("A 10 10", ellipse))), vec!["arrow-near-dot"]);
        }
    }

    #[test]
    fn event_row_pin_requires_the_event_and_all_straights_on_row() {
        let row_dot = |name, x| dot(name, x, 80.0, "branch_join").replace("/>", " data-rv-line=\"19\"/>");
        let (source, target, r, hr) = (row_dot("r2", 220.0), row_dot("s", 20.0), row_dot("r", 150.0), row_dot("h.r", 60.0));
        let path = r#"<g data-rv-id="r2>s" data-rv-kind="arrow" data-rv-var="r2" data-rv-to="s" data-rv-line="19" data-rv-event="static_die">
            <path class="arrow-shaft" d="M 214.75 80 L 160 80 A 10 10 0 0 0 140 80 L 70 80 A 10 10 0 0 0 50 80 L 38 80"/>
            <polygon class="arrow-head" points="38,74 25.25,80 38,86"/></g>"#;
        let matches = |source: &str, target: &str, r: &str, hr: &str, path: &str| {
            let svg = format!("<svg>{source}{target}{r}{hr}{path}</svg>");
            let doc = roxmltree::Document::parse(&svg).unwrap();
            parse(&doc).arrow_stays_on_row("r2", "s", "19", "static_die", &["r", "h.r"])
        };
        assert!(matches(&source, &target, &r, &hr, path));
        let wrong_row = |mark: &str| mark.replace("line=\"19\"", "line=\"18\"");
        let accepted: Vec<_> = [
            ("wrong source row", matches(&wrong_row(&source), &target, &r, &hr, path)),
            ("wrong target row", matches(&source, &wrong_row(&target), &r, &hr, path)),
            ("wrong crossing row", matches(&source, &target, &wrong_row(&r), &hr, path)),
            ("missing crossing", matches(&source, &target, &r, &hr, &path.replace("A 10 10 0 0 0 50 80", "L 50 80"))),
            ("wrong event", matches(&source, &target, &r, &hr, &path.replace("static_die", "move"))),
            ("short off-row track", matches(&source, &target, &r, &hr,
                &path.replace("L 70 80", "L 135 80 L 132 92 L 118 92 L 115 80 L 70 80"))),
            ("deep jog", matches(&source, &target, &r, &hr, &path.replace("L 160 80", "L 210 55 L 200 55 L 195 80 L 160 80"))),
        ].into_iter().filter_map(|(reason, matched)| matched.then_some(reason)).collect();
        assert!(accepted.is_empty(), "accepted invalid routes: {accepted:?}");
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
    fn a_head_along_a_column_is_caught() {
        // `let z = *c;` (basic_deref2, #1488): the copy dips two rows and
        // comes back up into z's first dot.
        let cols = [header("c"), header("z"), dot("c", 250.0, 185.0, "copy"), dot("z", 320.0, 185.0, "copy"),
                    dot("z", 320.0, 275.0, "owner_go_out_of_scope"), line("z", 320.0, 185.0, 275.0)].concat();
        // At 72 degrees its head lies along z's column for 10 px.
        let steep = arrow("c>z", "c", "z", "251.66 189.98 270 245 300 245 314.31 202.08",
                          "308.62,200.18 318.34,189.98 320,203.97");
        assert_eq!(invs(&[cols.clone(), steep].concat()), vec!["arrow-along-leg"]);
        // At 56 degrees (its last leg 40 px long), clear of it.
        let clear = arrow("c>z", "c", "z", "251.66 189.98 270 245 280 245 310.02 199.98",
                          "305.02,196.65 317.09,189.37 315.01,203.31");
        assert_eq!(invs(&[cols, clear].concat()), Vec::<&str>::new());
    }

    #[test]
    fn gutter_clearance_belongs_to_the_endpoint() {
        let col = [header("s"), dot("s", 70.0, 90.0, "acquire")].concat();
        let route = |pts| arrow("r>s", "r", "s", pts, "52,84 64.75,90 52,96");
        let hook = route("170 90 150 120 44 120 44 90 52 90");
        assert!(invs(&format!("{col}{hook}")).contains(&"arrow-in-gutter"));
        let stub = arrow("s>s", "s", "s", "64.75 90 52 90", "52,84 39.25,90 52,96");
        assert!(invs(&format!("{col}{stub}")).contains(&"arrow-in-gutter"));
        let from_logo = route("20 90 52 90");
        assert!(invs(&format!("{col}{from_logo}")).contains(&"arrow-in-gutter"));
        let logo = r#"<text x="8" y="95" data-rv-id="call" data-rv-kind="fn"
            data-rv-var="r" data-rv-to="s" data-rv-line="1"/>"#;
        assert!(!invs(&format!("{}{col}{from_logo}{logo}", header("r"))).contains(&"arrow-in-gutter"));

        let caller = arrow("r>caller", "r", "caller", "75 90 42 90 42 78", "36,78 42,65.25 48,78")
            .replace("data-rv-kind=\"arrow\"", "data-rv-kind=\"arrow\" data-rv-event=\"move\"");
        assert!(!invs(&format!("{col}{caller}")).contains(&"arrow-in-gutter"));
        let bad_column_end = arrow("r>s", "r", "s", "20 90 37 90", "37,84 49.75,90 37,96");
        assert!(invs(&format!("{col}{bad_column_end}{logo}")).contains(&"arrow-in-gutter"));

        // The logo identifies only the endpoint next to its glyph.
        let far_logo = logo.replace("x=\"8\"", "x=\"155\"");
        let bad_head = arrow("r>s", "r", "s", "152 90 50 90 50 120", "49,116 40,120 51,126");
        assert!(invs(&format!("{col}{bad_head}{far_logo}")).contains(&"arrow-in-gutter"));
        let bad_tail = arrow("s>f", "s", "f", "40 90 140 90", "140,84 152.75,90 140,96");
        let target_logo = far_logo.replace("var=\"r\"", "var=\"s\"").replace("to=\"s\"", "to=\"f\"");
        assert!(invs(&format!("{col}{bad_tail}{target_logo}")).contains(&"arrow-in-gutter"));
    }

    #[test]
    fn outside_figure_includes_the_painted_shaft_and_joins() {
        let arc = |y: f64| format!(r#"<g data-rv-id="arc" data-rv-kind="arrow">
            <path class="arrow-shaft" d="M 10 {y} L 20 {y} A 10 10 0 0 1 40 {y} L 50 {y}"/></g>"#);
        assert_eq!(invs(&arc(5.0)), vec!["outside-figure"]);
        assert_eq!(invs(&arc(11.0)), vec!["outside-figure"]); // apex at y=1
        assert!(invs(&arc(12.5)).is_empty()); // outer stroke touches y=0
        let turn = arrow("r>s", "r", "s", "480 60 497.5 42.5 480 25", "480,25 470,20 478,30");
        assert_eq!(invs(&turn), vec!["outside-figure"]); // only the miter exceeds x=500
        let flat = arrow("r>s", "r", "s", "0 2.5 100 2.5", "100,0 110,2.5 100,5");
        assert!(invs(&flat).is_empty()); // butt cap flush with the left edge
        assert_eq!(invs(&flat.replace("0 2.5", "0 2")), vec!["outside-figure"]);
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

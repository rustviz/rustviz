// #1595: a struct some field of which an arm moves out keeps one line;
// its text follows rustc's facts on its fields: partly moved in the arm
// after the move, "Some of p's fields may have been moved out" after the
// conditional, never "the owner of the resource" while one may be.
struct Pair {
    a: String,
    b: String,
}

struct Inner {
    x: String,
    y: String,
}

struct Outer {
    inner: Inner,
    z: String,
}

fn whole(p: Pair, c: bool) {
    if c {
        let Pair { a, b } = p;
    }
}

fn one_field(q: Pair, c: bool) {
    if c {
        drop(q.a);
        let n = 1;
    } else {
        let j = 2;
    }
}

fn else_if(w: Pair, c: bool, d: bool) {
    let s2 = w.b;
    if c {
        drop(w.a);
    } else if d {
        drop(w.a);
    }
}

fn tuple(c: bool) {
    let t = (String::from("a"), String::from("b"));
    if c {
        drop(t.0);
        let n = 1;
    }
}

fn match_or(m: Pair, n: u8) {
    match n {
        0 => drop(m.a),
        1 | 2 => {
            let s = m.b;
            let j = 2;
        }
        _ => {}
    }
}

fn if_let(l: Pair, opt: Option<u8>) {
    if let Some(x) = opt {
        let s = l.a;
        let k = x + 1;
    }
}

fn nested(e: Pair, c: bool, d: bool) {
    if c {
        if d {
            drop(e.a);
            let k = 1;
        }
        let j = 2;
    }
}

fn leaves(r: Pair, c: bool) -> usize {
    if c {
        drop(r.a);
        return 0;
    }
    r.b.len()
}

fn both_arms(u: Pair, c: bool) {
    let s3 = u.b;
    if c {
        drop(u.a);
        let k = 1;
    } else {
        drop(u.a);
        let j = 2;
    }
}

fn deep(o: Outer, c: bool) {
    if c {
        drop(o.inner.x);
        let k = 1;
    }
}

fn given_back(mut g: Pair, c: bool) {
    if c {
        drop(g.a);
        g.a = String::from("c");
    }
    let n = g.a.len();
}

fn chain(h: Pair, opt: Option<u8>) {
    if let Some(x) = opt && x > 1 {
        drop(h.a);
        let k = 1;
    }
}

fn borrowed(v: Pair, c: bool) {
    if c {
        let r = &v;
        let n = r.a.len();
        drop(v.a);
    } else {
        let j = 2;
    }
}

fn let_else(f: Pair, opt: Option<u8>) {
    let Some(x) = opt else {
        drop(f.a);
        return;
    };
    let n = f.a.len() + x as usize;
}

fn match_self(y: Pair, c: bool) {
    match y {
        Pair { a, .. } if c => drop(a),
        _ => {}
    }
}

fn value_if(z: Pair, c: bool) -> String {
    let s = if c { z.a } else { z.b };
    s
}

struct Held<'a> {
    r: &'a mut i32,
    n: i32,
}

fn two_ifs(mut d: Pair, c: bool, e: bool) {
    if c { drop(d.a); d.a = String::new(); } if e {
        drop(d.a);
    }
    let n = 1;
}

fn ref_field(hh: Held, c: bool) {
    if c {
        let r = hh.r;
        *r += 1;
    }
}

fn or_alt(ta: (Option<String>, Option<String>)) {
    if let (Some(x), _) | (_, Some(x)) = ta {
        let k = x.len();
    }
}

// Review cases (#1646): a field only part of which is moved out (an array
// element, an enum field's payload) doesn't "still hold its value".
struct Arr {
    a: String,
    tag: String,
    xs: [String; 2],
}

struct Opt {
    a: String,
    opt: Option<String>,
}

fn array_part(ap: Arr) {
    drop(ap.a);
    let [head, _] = ap.xs;
    let n = ap.tag.len();
}

fn enum_part(op: Opt) {
    drop(op.a);
    if let Some(s) = op.opt {
        let k = s.len();
    }
}

// Two variables of one name share a column; each has its own facts.
fn two_blocks() {
    {
        let bp = Pair { a: String::new(), b: String::new() };
        let n = bp.b.len();
        drop(bp.a);
    }
    {
        let bp = Pair { a: String::new(), b: String::new() };
        let n = bp.a.len();
        drop(bp.b);
    }
}

// The else arm's comment row: nothing is moved on that way.
fn else_comment(ec: Pair, c: bool) {
    if c {
        drop(ec.a);
    } else {
        // nothing here
    }
}

fn use_ref(_p: &Pair) {}

// A move after the join, on its line: not one of the branches'.
fn after_join(aj: Pair, c: bool) {
    if c {
        use_ref(&aj);
    } else {
        let j = 2;
    } drop(aj.a);
}

// The whole struct borrowed on the split line before the arm's move.
fn split_line(sl: Pair, c: bool) {
    if c { let r = &sl; use_ref(r); drop(sl.a);
        let k = 1;
    }
}

// A loop's body: its rows, not its closing brace.
fn loop_refill(mut lr: Pair) {
    loop {
        drop(lr.a);
        let n = lr.b.len();
        lr.a = String::new();
        break;
    }
}

// An `else` that never runs moves nothing.
#[allow(irrefutable_let_patterns)]
fn dead_else(de: Pair) {
    if let _ = () {
        let k = 1;
    } else {
        drop(de.a);
    }
    let n = de.b.len();
}

fn main() {
    let mk = || Pair { a: String::from("a"), b: String::from("b") };
    whole(mk(), true);
    one_field(mk(), true);
    else_if(mk(), true, false);
    tuple(true);
    match_or(mk(), 0);
    if_let(mk(), Some(1));
    nested(mk(), true, false);
    leaves(mk(), true);
    both_arms(mk(), true);
    deep(Outer { inner: Inner { x: String::from("x"), y: String::from("y") }, z: String::from("z") }, true);
    given_back(mk(), true);
    chain(mk(), Some(2));
    borrowed(mk(), true);
    let_else(mk(), Some(1));
    match_self(mk(), true);
    value_if(mk(), true);
    two_ifs(mk(), true, false);
    let mut i = 1;
    ref_field(Held { r: &mut i, n: 2 }, true);
    or_alt((Some(String::from("a")), None));
    array_part(Arr { a: String::new(), tag: String::new(), xs: [String::new(), String::new()] });
    enum_part(Opt { a: String::new(), opt: Some(String::new()) });
    two_blocks();
    else_comment(mk(), true);
    after_join(mk(), true);
    split_line(mk(), true);
    loop_refill(mk());
    dead_else(mk());
}

// RustViz 3 fact layer: the cases the review of #1850 found, pinned by
// `facts_pin_review_1850` in rustviz-lib/tests/corpus.rs.
struct P {
    a: String,
    b: String,
}

struct H {
    s: String,
    t: String,
}

mod m {
    pub struct Q {
        pub x: String,
        secret: String,
    }
    impl Q {
        pub fn new() -> Q {
            Q { x: String::new(), secret: String::new() }
        }
    }
}

fn box_order(c: bool) {
    let mut b = Box::new(String::from("ab"));
    if c {
        let s = *b;
    }
    b = Box::new(String::from("cd"));
}

fn moved_ref(mut p: P) {
    let r = &mut p;
    let t = r;
    t.b.push('!');
}

fn sibling_scopes(c: bool) {
    if c {
        let p = P { a: String::new(), b: String::new() };
        let x = p.a;
    } else {
        let p = P { a: String::new(), b: String::new() };
        let y = p.b;
    }
}

fn nested_box() {
    let boxed = Box::new(Box::new(String::from("t")));
    let text = **boxed;
}

fn exit_payload(c: bool, d: bool) -> u8 {
    let b = Box::new(String::from("x"));
    if c {
        return 1;
    }
    if d {
        let s = *b;
        return 2;
    }
    0
}

fn boxed_fields(b: Box<H>) {
    let x = &b.s;
    let n = 1;
    println!("{} {}", x, n);
}

fn private_field(q: m::Q) {
    let n = q.x.len();
}

fn double_ref(r: &&P) {
    let n = r.a.len();
}

fn main() {
    box_order(true);
    moved_ref(P { a: String::new(), b: String::new() });
    sibling_scopes(true);
    nested_box();
    exit_payload(false, true);
    boxed_fields(Box::new(H { s: String::new(), t: String::new() }));
    private_field(m::Q::new());
    double_ref(&&P { a: String::new(), b: String::new() });
}

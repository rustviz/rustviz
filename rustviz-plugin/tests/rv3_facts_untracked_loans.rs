// RustViz 3 facts (P2-1a): loans borrowck doesn't track (what a reference
// parameter holds, a reference to a constant, a shared reborrow through a
// shared reference), reborrow chains (`made_through`), the places whose own
// reference a loan is (`direct_holders`) and holders by a call's result
// (`via_call`). Pinned by facts_pin_untracked_loans in corpus.rs.
fn copy_param(p: &String) -> usize {
    let q = p;
    let k = 1;
    q.len() + k
}

fn reborrow_shared(s: &String) -> usize {
    let r = &*s;
    let b = s.as_bytes();
    let n = r.len();
    n + b.len()
}

fn id(s: &str) -> &str {
    s
}

fn unused(p: &i32) {
    let z = 1;
    println!("{}", z);
}

fn early(p: &i32, c: bool) -> i32 {
    if c {
        return 0;
    }
    *p
}

struct H<'a> {
    r: &'a String,
}

fn holder<'a>(s: &'a String) -> H<'a> {
    H { r: s }
}

fn main() {
    let x = 5;
    let y = &x;
    let z = y;
    println!("{}", z);
    let t = "hi";
    let u = t;
    println!("{} {}", t, u);
    let mut s = String::new();
    let a = &mut s;
    let b = &mut *a;
    b.push('x');
    let c = &*a;
    println!("{}", c);
    a.push('y');
    let m = &s;
    let h = holder(m);
    let o = Some(m);
    println!("{} {:?}", h.r, o);
    let w = String::from("a b");
    let bytes = w.as_bytes();
    println!("{}", bytes[0]);
    copy_param(&w);
    reborrow_shared(&w);
    id("x");
    unused(&1);
    early(&1, true);
}

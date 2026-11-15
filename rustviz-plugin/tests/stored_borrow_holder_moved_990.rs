// #990: after `keep(&mut v, &s)` may store `&s` in v (#513), moving v
// on (`let w = v;`) moves the stored borrow with it: s stays shared
// until w's last use (`w.len()`), so rustc rejects moving s
// (`let s2 = s;`) anywhere before it. A later shared borrow of s that
// ends before then doesn't end s's borrow: w still holds it. Main
// stopped tracking the borrow at v's move and said "s's resource is no
// longer immutably borrowed" after `look(&s)`.
fn keep<'a>(hold: &mut Vec<&'a String>, r: &'a String) {
    hold.push(r);
}

fn keep_mut<'a>(hold: &mut Vec<&'a mut String>, r: &'a mut String) {
    hold.push(r);
}

fn pass<'a>(v: Vec<&'a String>) -> Vec<&'a String> {
    v
}

fn look(r: &String) -> usize {
    r.len()
}

struct H<'a> {
    v: Vec<&'a String>,
}

fn main() {
    let s = String::from("s");
    let mut v = Vec::new();
    keep(&mut v, &s);
    let w = v;
    let n = look(&s);
    println!("{} {}", w.len(), n);
}

// A reference whose last use falls between the move and `w2.len()`.
fn reference() {
    let t = String::from("t");
    let mut v2 = Vec::new();
    keep(&mut v2, &t);
    let w2 = v2;
    let r = &t;
    println!("{}", r);
    println!("{}", w2.len());
}

// Moved twice, into a struct field, and through a call whose
// signature gives its result the argument's lifetime.
fn moved_on() {
    let a = String::from("a");
    let mut va = Vec::new();
    keep(&mut va, &a);
    let wa = va;
    let xa = wa;
    let na = look(&a);
    println!("{} {}", xa.len(), na);

    let b = String::from("b");
    let mut vb = Vec::new();
    keep(&mut vb, &b);
    let h = H { v: vb };
    let nb = look(&b);
    println!("{} {}", h.v.len(), nb);

    let c = String::from("c");
    let mut vc = Vec::new();
    keep(&mut vc, &c);
    let wc = pass(vc);
    let nc = look(&c);
    println!("{} {}", wc.len(), nc);
}

// A `&mut` borrow moves on the same way; after w4's last use, a later
// borrow of d is the only one.
fn mutable() {
    let mut m = String::from("m");
    let mut v3 = Vec::new();
    keep_mut(&mut v3, &mut m);
    let w3 = v3;
    println!("{}", w3.len());
    m.push('!');

    let d = String::from("d");
    let mut v4 = Vec::new();
    keep(&mut v4, &d);
    let w4 = v4;
    println!("{}", w4.len());
    let nd = look(&d);
    println!("{}", nd);
}

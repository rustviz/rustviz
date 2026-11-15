// #1015: after `keepv(&mut v, &s)` may store `&s` in v (#513), a value
// made from v whose type rustc ties to that reference's lifetime holds
// it too: `let w: Vec<&String> = v.iter().copied().collect();` copies
// the reference into w, so w keeps s borrowed until its last use. Main
// followed only moves, copies and clones (#1003) of v, so `look(&s)`
// named v, which isn't used after the `collect`, and keepv's text left
// w out. rustc rejects moving each lender right after each `look` below
// (borrow later used by the derived value), except in `not_held`.
fn keepv<'a>(hold: &mut Vec<&'a String>, r: &'a String) {
    hold.push(r);
}

fn look(r: &String) -> usize {
    r.len()
}

fn firsts<'a>(v: &[&'a String]) -> Vec<&'a String> {
    v.iter().take(1).copied().collect()
}

fn main() {
    let s = String::from("s");
    let mut v = Vec::new();
    keepv(&mut v, &s);
    let w: Vec<&String> = v.iter().copied().collect();
    let n = look(&s);
    println!("{} {}", w.len(), n);
}

// An iterator over v reaches the reference through v.
fn iter_held() {
    let a = String::from("a");
    let mut va = Vec::new();
    keepv(&mut va, &a);
    let it = va.iter();
    let na = look(&a);
    println!("{} {}", it.count(), na);
}

// A reference copied out of v, in an Option or on its own.
fn taken_out() {
    let b = String::from("b");
    let mut vb = Vec::new();
    keepv(&mut vb, &b);
    let fb = vb.first().copied();
    let nb = look(&b);
    println!("{:?} {}", fb, nb);
    let c = String::from("c");
    let mut vc = Vec::new();
    keepv(&mut vc, &c);
    let rc = vc[0];
    let nc = look(&c);
    println!("{} {}", rc, nc);
}

// Through a function whose signature ties its result to v's references.
fn via_fn() {
    let d = String::from("d");
    let mut vd = Vec::new();
    keepv(&mut vd, &d);
    let wd = firsts(&vd);
    let nd = look(&d);
    println!("{} {}", wd.len(), nd);
}

// Both v and the value made from it in use after look.
fn both() {
    let e = String::from("e");
    let mut ve = Vec::new();
    keepv(&mut ve, &e);
    let we: Vec<&String> = ve.iter().copied().collect();
    let ne = look(&e);
    println!("{} {} {}", ve.len(), we.len(), ne);
}

// Made from a clone of v: named with the clone.
fn of_clone() {
    let f = String::from("f");
    let mut vf = Vec::new();
    keepv(&mut vf, &f);
    let wf = vf.clone();
    let xf: Vec<&String> = wf.iter().copied().collect();
    let nf = look(&f);
    println!("{} {}", xf.len(), nf);
}

// A struct with its own Drop: dg's drop at the end of the fn may use
// the reference, so g stays borrowed until then.
struct D<'a> {
    v: Vec<&'a String>,
}

impl Drop for D<'_> {
    fn drop(&mut self) {}
}

fn into_drop() {
    let g = String::from("g");
    let mut vg = Vec::new();
    keepv(&mut vg, &g);
    let dg = D { v: vg.iter().copied().collect() };
    let ng = look(&g);
    println!("{} {}", dg.v.len(), ng);
    println!("{}", ng);
}

// A value made from v that holds no reference: the borrow ends at v's
// last use, and look's borrow is the only one.
fn not_held() {
    let k = String::from("k");
    let mut vk = Vec::new();
    keepv(&mut vk, &k);
    let lens: Vec<usize> = vk.iter().map(|r| r.len()).collect();
    let nk = look(&k);
    println!("{} {}", lens.len(), nk);
}

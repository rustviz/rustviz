// #926: `&mut gm` with gm a `&mut String`, coerced to `&mut String`, is
// a reborrow of `*gm` (rustc derefs twice), so its arrow reads like the
// written `&mut *gm`, not like a borrow of gm itself.
fn add(s: &mut String) {
    s.push('!');
}

fn let_coerced() {
    let mut c = String::from("c");
    let mut gm = &mut c;
    let s: &mut String = &mut gm;
    s.push('#');
    gm.push('$');
}

// The same at a call: `add(&mut gm)` becomes `add(&mut **(&mut gm))`.
fn call_coerced() {
    let mut d = String::from("d");
    let mut gn = &mut d;
    add(&mut gn);
    gn.push('$');
}

// Written out, for comparison.
fn let_written() {
    let mut e = String::from("e");
    let ge = &mut e;
    let t = &mut *ge;
    t.push('#');
    ge.push('$');
}

fn main() {
    let_coerced();
    call_coerced();
    let_written();
}

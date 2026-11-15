// #939: `&gx` with gx a `&String`, coerced to `&String`, is a reborrow
// of `*gx` (rustc derefs twice), so its arrow reads like the written
// `&*x2`, not like a borrow of gx itself. The shared twin of #926.
fn show_str(s: &String) -> usize {
    s.len()
}

fn let_coerced() {
    let c = String::from("c");
    let gx = &c;
    let s: &String = &gx;
    println!("{}", s);
}

// Written out, for comparison.
fn let_written() {
    let b = String::from("b");
    let x2 = &b;
    let q = &*x2;
    println!("{}", q);
}

// `&gm` with gm a `&mut String`, coerced to `&String`: a shared
// reborrow of `*gm`, like the written `&*gw`.
fn let_coerced_of_mut() {
    let mut f = String::from("f");
    let gm = &mut f;
    let t: &String = &gm;
    println!("{}", t);
    gm.push('!');
}

fn let_written_of_mut() {
    let mut h = String::from("h");
    let gw = &mut h;
    let u = &*gw;
    println!("{}", u);
    gw.push('!');
}

// At a call: `show_str(&gy)` becomes `show_str(&**(&gy))`.
fn call_coerced() {
    let d = String::from("d");
    let gy = &d;
    let n = show_str(&gy);
    println!("{} {}", n, gy);
}

// `&mut gv` unsized to a `&mut dyn Write` derefs only once: dv points
// to gv itself, so this is no reborrow: gv itself is borrowed, and
// what dv borrows is the reference gv holds, a value (#947, as for
// the shared `&gd` unsized to `&dyn Debug`).
fn mut_unsized() {
    let mut w = Vec::<u8>::new();
    let mut gv = &mut w;
    let dv: &mut dyn std::io::Write = &mut gv;
    dv.write_all(b"x").unwrap();
}

fn main() {
    let_coerced();
    let_written();
    let_coerced_of_mut();
    let_written_of_mut();
    call_coerced();
    mut_unsized();
}

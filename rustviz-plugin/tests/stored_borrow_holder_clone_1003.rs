// #1003: after `keepv(&mut v, &s)` may store `&s` in v (#513), a clone
// of v (`let w = v.clone();`, with v: Vec<&String>) holds a copy of
// that reference, so it keeps s borrowed too: v and w each hold the
// borrow until their own last use (a Vec's drop doesn't use the
// references in it). Main named v at `look(&s)` although only w is used
// after it, and left w out of keepv's text. rustc rejects moving each
// lender right after each `look` below.
fn keepv<'a>(hold: &mut Vec<&'a String>, r: &'a String) {
    hold.push(r);
}

fn look(r: &String) -> usize {
    r.len()
}

fn main() {
    let s = String::from("s");
    let mut v = Vec::new();
    keepv(&mut v, &s);
    let w = v.clone();
    let n = look(&s);
    println!("{} {}", w.len(), n);
}

// Both v and its clone in use after look.
fn both() {
    let a = String::from("a");
    let mut va = Vec::new();
    keepv(&mut va, &a);
    let wa = va.clone();
    let na = look(&a);
    println!("{} {} {}", va.len(), wa.len(), na);
}

// The other spellings of a clone.
fn spellings() {
    let b = String::from("b");
    let mut vb = Vec::new();
    keepv(&mut vb, &b);
    let wb = Clone::clone(&vb);
    let nb = look(&b);
    println!("{} {}", wb.len(), nb);
    let c = String::from("c");
    let mut vc = Vec::new();
    keepv(&mut vc, &c);
    let wc = vc.to_vec();
    let nc = look(&c);
    println!("{} {}", wc.len(), nc);
}

// A clone of a struct holding the borrow in a field.
#[derive(Clone)]
struct H<'a> {
    v: Vec<&'a String>,
}

fn field() {
    let t = String::from("t");
    let mut h = H { v: Vec::new() };
    keepv(&mut h.v, &t);
    let h2 = h.clone();
    let nt = look(&t);
    println!("{} {}", h2.v.len(), nt);
}

// The clone moves on: the borrow goes with it.
fn clone_then_move() {
    let m = String::from("m");
    let mut vm = Vec::new();
    keepv(&mut vm, &m);
    let wm = vm.clone();
    let xm = wm;
    let nm = look(&m);
    println!("{} {}", xm.len(), nm);
}

// A type with its own Drop: its drop at the end of the fn may use the
// reference, so d and its clone e both hold the borrow until then.
#[derive(Clone)]
struct D<'a> {
    v: Vec<&'a String>,
}

impl Drop for D<'_> {
    fn drop(&mut self) {}
}

fn keepd<'a>(d: &mut D<'a>, r: &'a String) {
    d.v.push(r);
}

fn with_drop() {
    let g = String::from("g");
    let mut d = D { v: Vec::new() };
    keepd(&mut d, &g);
    let e = d.clone();
    let ng = look(&g);
    println!("{} {}", e.v.len(), ng);
}

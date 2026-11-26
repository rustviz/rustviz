// RustViz 3 facts: what the review of #1854 (P2-1a) found wrong in the
// loan facts, each fixed from MIR. Pinned by facts_pin_review_1854 in
// corpus.rs.

// `k.1` holds b's reference (a was moved into `k.0`): the reborrow in
// `k.1.push` is made through k.1, so only b's loan is its chain.
fn pair_field(a: &mut String, b: &mut String) {
    let k = (a, b);
    k.1.push('!');
}

// `*z = &c` stores the loan of c in y, through z: z itself still holds
// `&mut y`.
fn store_through() {
    let c = 1;
    let d = 2;
    let mut y = &d;
    let z = &mut y;
    *z = &c;
    println!("{}", y);
}

struct Guard<'a> {
    r: &'a String,
}

impl Drop for Guard<'_> {
    fn drop(&mut self) {
        println!("{}", self.r);
    }
}

// g's drop reads `self.r`: the caller's loan is in use until g is dropped.
fn guard(p: &String) {
    let g = Guard { r: p };
    let n = 1;
    println!("{}", n);
}

// The reference is stored in the caller's vector: the caller's loan isn't
// given back in this body.
fn keep<'a>(hold: &mut Vec<&'a String>, r: &'a String) {
    hold.push(r);
}

struct Hold<'a> {
    r: &'a String,
}

// Stored in the caller's struct, through h.
fn point<'a>(h: &mut Hold<'a>, t: &'a String) {
    h.r = t;
}

// Each literal is held by its own field; `let q = p.0;` copies "left" only.
fn consts() {
    let p = ("left", "right");
    let q = p.0;
    println!("{} {}", q, p.1);
}

struct HS<'a> {
    r: &'a str,
}

// `h.r = "y"` replaces the "x" the field held: "x" isn't held after it.
fn overwrite() {
    let mut h = HS { r: "x" };
    println!("{}", h.r);
    h.r = "y";
    println!("{}", h.r);
}

// `h.0 = b` overwrites the field holding a's reference; h.1 keeps it.
fn hold_fields(a: &String, b: &String) -> usize {
    let mut h = (a, a);
    h.0 = b;
    h.1.len() + h.0.len()
}

// On the path that goes round again, y is done with its element while the
// iterator keeps the loan; on the `break` path the loan ends.
fn first_long(v: &Vec<String>) -> usize {
    let mut n = 0;
    for y in v.iter() {
        if y.len() > 3 { break; }
        n += 1;
    }
    n
}

// p is never used: the caller's loan ends on each way out.
fn unused_two(p: &i32, c: bool) -> i32 {
    if c {
        return 1;
    }
    2
}

// The loan of `*r0` reaches o6.0 on the sixth time round (an `Option`
// is copied, a reference would be reborrowed).
fn chain(r0: &String) -> usize {
    let mut o1: Option<&String> = None;
    let mut o2: Option<&String> = None;
    let mut o3: Option<&String> = None;
    let mut o4: Option<&String> = None;
    let mut o5: Option<&String> = None;
    let mut o6: Option<&String> = None;
    for _ in 0..6 {
        o6 = o5;
        o5 = o4;
        o4 = o3;
        o3 = o2;
        o2 = o1;
        o1 = Some(r0);
    }
    let mut n = o1.map_or(0, |s| s.len());
    n += o2.map_or(0, |s| s.len());
    n += o3.map_or(0, |s| s.len());
    n += o4.map_or(0, |s| s.len());
    n += o5.map_or(0, |s| s.len());
    n + o6.map_or(0, |s| s.len())
}

// The `for` loop's hidden iterator keeps the caller's slice.
fn sum(xs: &[u32]) -> u32 {
    let mut t = 0;
    for x in xs {
        t += x;
    }
    t
}

// An auto-ref of a variable whose name starts with `addr_of`.
fn addr(addr_of_book: String) -> usize {
    addr_of_book.len()
}

const BUF: &mut [u8] = {
    let x: &mut [u8] = &mut [];
    x
};

// A constant `&mut` reference: kept by b, a mutable reference.
fn mut_const() -> usize {
    let b = BUF;
    b.len()
}

fn make_res() -> Result<String, ()> {
    Ok(String::new())
}

// `ref s` borrows the `Ok` payload of the temporary `make_res()`.
fn temp_name() {
    if let Ok(ref s) = make_res() {
        println!("{}", s);
    }
}

// `if let` on `v.get_mut(0)`: every loan a chain names is listed.
fn get_mut_else() {
    let mut v = vec![String::from("b")];
    if let Some(b) = v.get_mut(0) {
        b.push('!');
    } else {
        println!("no v");
    }
}

// `Holder { r: &*m }`: one borrow written, and rustc's reborrow of it.
fn reborrow_field(r: &String, m: &mut String) {
    let mut h = Hold { r: &*m };
    h.r = r;
    println!("{}", h.r);
}

// `Some(m)` copies m into o.0; `if let Some(x) = o` copies it out into x.
fn pattern_copy(s: &String) -> usize {
    let m = s;
    let o = Some(m);
    if let Some(x) = o {
        return x.len();
    }
    0
}

fn main() {
    let mut a = String::new();
    let mut b = String::new();
    pair_field(&mut a, &mut b);
    store_through();
    guard(&a);
    let mut v = Vec::new();
    keep(&mut v, &a);
    let mut h = Hold { r: &a };
    point(&mut h, &b);
    consts();
    overwrite();
    hold_fields(&a, &b);
    first_long(&vec![a.clone()]);
    unused_two(&1, true);
    chain(&a);
    sum(&[1, 2]);
    addr(String::new());
    mut_const();
    temp_name();
    get_mut_else();
    reborrow_field(&a, &mut b.clone());
    pattern_copy(&a);
}

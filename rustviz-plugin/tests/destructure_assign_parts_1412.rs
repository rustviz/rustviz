// #1412: a destructuring assignment whose right-hand side is a struct or
// tuple variable with field columns moves each field into its own place,
// as `let Pair { a, b } = p;` does: `p.a` into `s`, `p.b` into `t`. Those
// fields were said to be dropped at `p`'s scope end, though `s` and `t`
// own them. A field the pattern leaves (`..`, `_`) stays in the struct.
// A Copy field is copied, a reference field's reference too.
struct Pair {
    a: String,
    b: String,
}

struct W(String, i32);

struct Holder<'a> {
    r: &'a String,
    n: String,
}

fn whole() {
    let mut s = String::from("s");
    let mut t = String::from("t");
    println!("{} {}", s, t);
    let p = Pair { a: String::from("a"), b: String::from("b") };
    Pair { a: s, b: t } = p;
    println!("{} {}", s, t);
}

fn rest() {
    let mut u = String::from("u");
    println!("{}", u);
    let q = Pair { a: String::from("a"), b: String::from("b") };
    Pair { a: u, .. } = q;
    println!("{} {}", u, q.b);
}

fn tuple_wild() {
    let mut v = String::from("v");
    println!("{}", v);
    let tp = (String::from("a"), String::from("b"));
    (_, v) = tp;
    println!("{} {}", v, tp.0);
}

fn tuple_struct() {
    let mut w0 = String::from("w");
    let mut n = 0;
    println!("{} {}", w0, n);
    let w = W(String::from("a"), 5);
    W(w0, n) = w;
    println!("{} {}", w0, n);
}

fn in_arm() {
    let mut x = String::from("x");
    let mut y = String::from("y");
    println!("{} {}", x, y);
    let r = Pair { a: String::from("a"), b: String::from("b") };
    if x.len() > 0 {
        Pair { a: x, b: y } = r;
    }
    println!("{} {}", x, y);
}

// A reference field: x gets a copy of hold.r's reference and borrows
// what it borrows (bb), not hold.
fn ref_field() {
    let aa = String::from("a");
    let bb = String::from("b");
    let mut x = &aa;
    let mut m = String::new();
    println!("{} {}", x, m);
    let hold = Holder { r: &bb, n: String::from("n") };
    Holder { r: x, n: m } = hold;
    println!("{} {}", x, m);
}

fn main() {
    whole();
    ref_field();
    rest();
    tuple_wild();
    tuple_struct();
    in_arm();
}

// #1163: a value with nothing to drop is a "value", not a "resource",
// and a drop that drops nothing isn't drawn.
//
// The temporary a `let` literal builds holds only what the pattern
// doesn't bind by value. When none of that needs drop (here `P`, with
// just an i32 in it), nothing is dropped at the end of the `let`: no
// `(temporary)` column and no drop triangle. With a String left over
// (`keep_string`) the temporary is still drawn and dropped.
struct P { a: i32 }
struct Q { a: String, b: P }
struct W(String, P);
fn mk() -> P { P { a: 1 } }

fn lit_tuple(s: String, p: P) {
    let (x, _) = (s, p);
    println!("{}", x);
}

fn lit_struct(s: String, p: P) {
    let Q { a, .. } = Q { a: s, b: p };
    println!("{}", a);
}

fn lit_ctor(s: String, p: P) {
    let W(a, _) = W(s, p);
    println!("{}", a);
}

fn lit_branch(c: bool, f: String, g: String, p: P, q: P) {
    let (a, _) = if c { (f, p) } else { (g, q) };
    println!("{}", a);
}

fn lit_nested(s: String, p: P, q: P) {
    let ((a, _), _) = ((s, p), q);
    println!("{}", a);
}

fn lit_fresh(s: String) {
    let (a, _) = (s, mk());
    println!("{}", a);
}

fn keep_string(s: String, t: String) {
    let (a, _) = (s, t);
    println!("{}", a);
}

// What a reference stored in an element points to is an i32: copying
// it copies a value.
fn copy_through_element() {
    let a = 1;
    let b = 2;
    let slots: Vec<&i32> = vec![&a, &b];
    let n = *slots[0];
    println!("{}", n);
}

// The closure reborrows `*r`, an i32.
fn capture_through_mut_ref() {
    let mut n = 1;
    let r = &mut n;
    let mut f = || *r += 1;
    f();
}

fn bump(r: &mut i32) -> bool { *r += 1; *r > 2 }

// The else branch of a let-chain that lent `*r` (an i32) to bump.
fn chain(r: &mut i32, o: Option<i32>) {
    if let Some(x) = o && bump(r) && x > 1 {
        println!("{}", x);
    } else {
        println!("no");
    }
}

fn main() {
    lit_tuple(String::new(), mk());
    lit_struct(String::new(), mk());
    lit_ctor(String::new(), mk());
    lit_branch(true, String::new(), String::new(), mk(), mk());
    lit_nested(String::new(), mk(), mk());
    lit_fresh(String::new());
    keep_string(String::new(), String::new());
    copy_through_element();
    capture_through_mut_ref();
    let mut k = 0;
    chain(&mut k, Some(3));
}

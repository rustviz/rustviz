// #1282: a Box holding a reference. `k1.push('!')` with `k1: Box<&mut
// String>` is `String::push(&mut **k1, '!')`: it writes s's String
// through the `&mut` in k1's Box (k1 needn't be `mut`), and doesn't
// borrow the Box itself. Dropping k1 frees the Box, not s's String.
struct Pair<'a> { f: &'a mut String }
fn wrap<T>(x: T) -> Box<T> { Box::new(x) }
fn take_box(b: Box<&mut String>) { println!("{}", b.len()); }

// A `&mut` variable moved into the Box.
fn moved_in() {
    let mut s1 = String::from("a");
    let r1 = &mut s1;
    let k1 = Box::new(r1);
    k1.push('!');
    println!("{}", k1);
}

// `&mut s` written in place.
fn borrow_in_place() {
    let mut s2 = String::from("b");
    let k2 = Box::new(&mut s2);
    k2.push('!');
    println!("{}", s2);
}

// A shared reference in the Box, read through it.
fn shared_in_box() {
    let s3 = String::from("c");
    let k3 = Box::new(&s3);
    println!("{}", k3.len());
}

// The Box moved on to another variable, then used.
fn moved_on() {
    let mut s4 = String::from("d");
    let r4 = &mut s4;
    let k4 = Box::new(r4);
    let k5 = k4;
    k5.push('!');
    println!("{}", s4);
}

// Moved into a call.
fn into_call() {
    let mut s6 = String::from("e");
    let r6 = &mut s6;
    let k6 = Box::new(r6);
    take_box(k6);
    println!("{}", s6);
}

// In an `if` arm.
fn in_arm(go: bool) {
    let mut s7 = String::from("f");
    let r7 = &mut s7;
    if go {
        let k7 = Box::new(r7);
        k7.push('!');
    }
    println!("{}", s7);
}

// A generic function returning the Box.
fn via_generic() {
    let mut s8 = String::from("g");
    let r8 = &mut s8;
    let k8 = wrap(r8);
    k8.push('!');
    println!("{}", s8);
}

// A reference parameter moved into the Box (the caller's loan going on
// in k9 isn't drawn yet: #1289).
fn param(r9: &mut String) {
    let k9 = Box::new(r9);
    k9.push('!');
}

// A struct field holding the `&mut` (already drawn as a reference).
fn in_struct() {
    let mut s10 = String::from("h");
    let r10 = &mut s10;
    let p10 = Pair { f: r10 };
    p10.f.push('!');
    println!("{}", s10);
}

fn main() {
    moved_in();
    borrow_in_place();
    shared_in_box();
    moved_on();
    into_call();
    in_arm(true);
    via_generic();
    let mut s = String::new();
    param(&mut s);
    in_struct();
}

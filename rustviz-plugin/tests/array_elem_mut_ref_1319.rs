// #1319: an array element holding a `&mut`. `k[0].push('!')` with `k:
// [&mut String; 1]` is `String::push(&mut *k[0], '!')`: it reborrows the
// String through the reference in k[0] (array indexing is built in, so k
// needn't be `mut`), and k can't be used while that reborrow is live. It
// doesn't borrow k's own value. (A `Vec` is different: `v[0].push('!')`
// goes through `IndexMut`, which mutably borrows v.)
fn takes(x: &mut String) { x.push('?'); }

// A local `&mut` moved into the array.
fn local() {
    let mut s = String::new();
    let r = &mut s;
    let k = [r];
    k[0].push('!');
    println!("{}", s);
}

// A `&mut` parameter moved into the array.
fn param(r: &mut String) {
    let k = [r];
    k[0].push('!');
}

// The element passed to a call (an implicit reborrow, `&mut *k[0]`).
fn call_arg(r: &mut String) {
    let k = [r];
    takes(k[0]);
}

// The reborrow written out.
fn explicit(r: &mut String) {
    let k = [r];
    takes(&mut *k[0]);
}

// A read through the `&mut` element.
fn read(r: &mut String) {
    let k = [r];
    let n = k[0].len();
    println!("{}", n);
}

// In an `if` arm.
fn in_arm(r: &mut String, c: bool) {
    let k = [r];
    if c {
        k[0].push('!');
    }
}

// k declared `mut` (it needn't be).
fn mut_array(r: &mut String) {
    let mut k = [r];
    k[0].push('!');
}

// Shared references in the array.
fn shared(s: &String) {
    let k = [s];
    let n = k[0].len();
    println!("{}", n);
}

fn main() {
    local();
    let mut s = String::new();
    param(&mut s);
    call_arg(&mut s);
    explicit(&mut s);
    read(&mut s);
    in_arm(&mut s, true);
    mut_array(&mut s);
    shared(&s);
}

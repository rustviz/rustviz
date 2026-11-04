// Writing through a `&mut` (`*r = v`) or a `Box` (`*b = v`) drops the
// value that was there when its type needs drop. The place has no column
// of its own, so the write's tooltip says so, in the words of a plain
// assignment (2026-11-05 decision): "*r acquires ownership of a new
// resource; its previous resource is dropped". `*n = 5` (an integer) is
// "*n is assigned a new value", `*rq = q2` (a struct with nothing to drop)
// "*rq acquires ownership of a resource", and `*r += "x"` (an operator
// call, nothing overwritten) keeps "modified through".
struct Q {
    n: i32,
}

fn replace(r: &mut String, s: String) {
    *r = s;
}

fn bump(n: &mut i32) {
    *n = 5;
}

fn append(r: &mut String) {
    *r += "x";
}

fn main() {
    let mut a = String::from("a");
    replace(&mut a, String::from("b"));
    let mut k = 1;
    bump(&mut k);
    append(&mut a);
    let mut s = String::from("s");
    let q = &mut s;
    *q = String::from("t");
    let mut b = Box::new(String::from("c"));
    *b = String::from("d");
    let mut q1 = Q { n: 1 };
    let q2 = Q { n: 2 };
    let rq = &mut q1;
    *rq = q2;
    println!("{} {} {} {} {}", a, k, s, b, q1.n);
}

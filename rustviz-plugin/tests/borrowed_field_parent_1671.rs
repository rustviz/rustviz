struct P { a: String, n: i32 }
fn main() {
    let mut p = P { a: String::from("a"), n: 1 };
    let r = &mut p.a;
    r.push('!');
    println!("{}", p.n);
    println!("{}", r);
}

struct Outer { inner: P }

fn nested(o: Outer) {
    let P { ref a, .. } = o.inner;
    println!("{}", a);
    let f = || o.inner.a.len();
    println!("{}", f());
}

struct List { items: Vec<i32>, count: usize }

fn iterate(mut list: List) {
    for n in list.items.iter_mut() {
        list.count += 1;
        *n += 1;
    }
}

struct Refs<'a> { shared: &'a i32, unique: &'a mut i32 }

fn reference_fields(h: Refs<'_>) {
    let slot = &h.shared;
    println!("{}", slot);
    let shared = &*h.shared;
    println!("{}", shared);
    let unique = &*h.unique;
    println!("{}", unique);
}

// A loan of the slot passed through a call ends with the call's result
// (x); `drop(h)` below compiles.
struct S<'a> { s: &'a i32 }
fn id<'a>(x: &'a &'a i32) -> &'a &'a i32 { x }
fn via_call(h: S<'_>) {
    let x = id(&h.s);
    println!("{x}");
    let n = 1;
    drop(h);
    println!("{n}");
}

// A mutable borrow of a reference field's slot locks the whole struct.
fn mut_slot<'a>(mut g: S<'a>, k: &'a i32) {
    let m = &mut g.s;
    *m = k;
    println!("{}", g.s);
}

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

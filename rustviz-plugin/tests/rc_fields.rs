use std::rc::Rc;
use std::sync::Arc;

struct P {
    shared: Rc<String>,
    n: i32,
}

struct Mixed {
    shared: Rc<String>,
    name: String,
}

struct Counter {
    hits: Arc<i32>,
}

fn main() {
    let a = Rc::new(String::from("hi"));
    let p = P { shared: Rc::clone(&a), n: 1 };
    let m = Mixed { shared: a.clone(), name: String::from("m") };
    let q = Rc::clone(&p.shared);
    let r = p.shared.clone();
    let c = Counter { hits: Arc::new(7) };
    println!("{} {} {} {} {} {} {}", p.n, m.name, q, r, c.hits, m.shared, a);
}

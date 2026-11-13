use std::sync::RwLock;

fn apply<F: Fn()>(f: &F) {
    f();
}

fn main() {
    let m = RwLock::new(0);
    let f = move || {
        *m.write().unwrap() += 1;
    };
    apply(&f);
    let g = f;
    g();
}

use std::sync::Mutex;

fn apply<F: Fn()>(f: &F) {
    f();
}

fn main() {
    let m = Mutex::new(0);
    let f = move || {
        *m.lock().unwrap() += 1;
    };
    apply(&f);
    let g = f;
    g();
}

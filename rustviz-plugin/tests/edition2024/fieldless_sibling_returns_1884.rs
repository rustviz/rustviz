fn see(_: &String) {}

fn check(mut s: String, present: bool, stop: bool) {
    let value = present.then_some(&s);
    if let Some(r) = value {
        if stop { see(r); return; } see(r);
    } else {
        s.clear();
    }
    s.clear();
}

fn main() {}

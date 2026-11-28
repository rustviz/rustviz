fn apply<F: FnOnce() -> R, R>(f: F) -> R {
    f()
}

fn run(v: Vec<String>) -> Option<usize> {
    let s = String::from("x");
    let c = move || s;
    if v.len() > 1 {
        apply(c);
    }
    let s3 = 'blk: {
    };
    Some(0)
}

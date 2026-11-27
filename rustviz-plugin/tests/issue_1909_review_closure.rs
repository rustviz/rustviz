fn skipped(s: String) {
    let c = move || s;
    let z = 'blk: {
        if false { break 'blk c(); }
        String::new()
    };
    drop(z);
}

fn nested(s: String) {
    let c = move || s;
    let z = 'blk: {
        if true { break 'blk Some(c()); }
        None
    };
    drop(z);
}

fn interrupted(s: String, b: bool) -> Option<String> {
    let c = move |_: ()| s;
    let z = 'blk: {
        if true { break 'blk Some(c(if b { return None; })); }
        None
    };
    z
}

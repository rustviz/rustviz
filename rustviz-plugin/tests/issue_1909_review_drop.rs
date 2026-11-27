fn self_replace(mut b: Box<String>) {
    *b = *b;
}

fn partial(p: (String, i32)) {
    if true { drop(p.0); } else { drop(p); }
}

fn fully_consumed(p: (String, String)) {
    if false { drop(p); } else { drop(p.0); drop(p.1); }
}

fn skipped_write(mut s: String) {
    if false { s = String::from("skip"); }
    drop(s);
}

fn shared() {
    let r;
    if true { r = std::rc::Rc::new(String::from("shared")); }
}

fn first_iteration() {
    let s;
    for _ in 0..2 {
        s = String::new();
        break;
    }
}

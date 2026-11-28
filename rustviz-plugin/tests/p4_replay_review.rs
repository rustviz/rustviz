fn reborrow_after_repoint() {
    let mut x = 0;
    let mut y = 1;
    let mut r = &mut x;
    let old = &*r;
    r = &mut y; let new = &mut *r; println!("{old}");
    *new = 4;
}

fn pick<'a>(a: &'a str, _: &'a str) -> &'a str { a }

fn successive_call_results(a: String, b: String) {
    let r = pick(&a, &b); let s = pick(r, &b);
    println!("{r} {s}");
}

fn partial_array_on_return(a: bool) {
    let v = [String::new(), String::new()];
    if a { let [q, _] = v; return; }
}

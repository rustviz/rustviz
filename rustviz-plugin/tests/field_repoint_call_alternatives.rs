struct Holder<'a> { r: &'a String }

fn pick<'a>(left: &'a String, right: &'a String, flag: bool) -> &'a String {
    if flag { left } else { right }
}

fn run(flag: bool, a: String, b: String, c: String) {
    let mut h = Holder { r: &a };
    h.r = &c; h.r = pick(&a, &b, flag);
    println!("{}", h.r);
    if flag { h.r = pick(&b, &c, flag); } else { h.r = &a; }
    println!("{}", h.r);
}

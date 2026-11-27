struct Holder<'a> {
    r: &'a String,
}

fn holder(s: &String) -> Holder<'_> {
    Holder { r: s }
}

fn longest<'a>(a: &'a String, b: &'a String) -> &'a String {
    if a.len() > b.len() { a } else { b }
}

fn main() {
    let s = String::from("rust");
    let t = String::from("hi");
    let h = holder(longest(&s, &t));
    println!("{}", h.r);
    std::hint::black_box(&h);
}

static GLOBAL: String = String::new();

fn static_holder(_: &String) -> Holder<'_> {
    Holder { r: &GLOBAL }
}

fn forwarded_static() {
    let s = String::from("rust");
    let t = String::from("hi");
    let h = static_holder(longest(&s, &t));
    std::hint::black_box(h.r);
    std::hint::black_box(&h);
    drop(t);
}

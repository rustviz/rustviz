fn longest<'a>(a: &'a String, b: &'a String) -> &'a String {
    if a.len() > b.len() { a } else { b }
}

fn main() {
    let s = String::from("hi");
    let o = Some(s.clone());
    let s2 = 'blk: {
        s.clone()
    };
    if let Some(x) = &o { let r = longest(&s2, &s); } else { }
}

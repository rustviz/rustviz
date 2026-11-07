struct Excerpt<'a> {
    part: &'a str,
}

fn first_sentence(s: &String) -> &str {
    &s[..3]
}

fn main() {
    let novel = String::from("Call me Ishmael. Some years ago...");
    let e = Excerpt { part: first_sentence(&novel) };
    println!("{}", e.part);
}

fn longest<'a>(x: &'a str, y: &'a str) -> &'a str {
    if x.len() > y.len() { x } else { y }
}

fn first(s: &String) -> &str {
    &s[..1]
}

fn main() {
    let a = String::from("abc");
    let b = String::from("de");
    let c = String::from("fgh");
    let mut r = longest(&a, &b);
    println!("{}", r);
    r = first(&c);
    println!("{}", r);
}

fn pick<'a>(a: &'a String, b: &'a String) -> Option<&'a String> {
    if a.len() > b.len() { Some(a) } else { Some(b) }
}
fn two<'a, 'b>(a: &'a String, b: &'b String) -> Option<(&'a String, &'b String)> {
    Some((a, b))
}
fn pick_str<'a>(a: &'a str, b: &'a str) -> Option<&'a str> {
    if a.len() > b.len() { Some(a) } else { Some(b) }
}
fn main() {
    let a = String::from("a");
    let b = String::from("bb");
    if let Some(x) = pick(&a, &b) {
        println!("{}", x);
    }
    let o = pick(&a, &b);
    match o {
        Some(y) => {
            println!("{}", y);
        }
        None => {}
    }
    if let Some((l, r)) = two(&a, &b) {
        println!("{} {}", l, r);
    }
    if let Some(z) = pick_str(&a, "literal") {
        println!("{}", z);
    }
}

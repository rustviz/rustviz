fn longest<'a>(a: &'a String, b: &'a String) -> &'a String {
    if a.len() > b.len() { a } else { b }
}

fn main() {
    let s = String::from("rust");
    let vr = vec![s.as_str(), "abc"];
    let r = longest(&s, &s);
    if vr.is_empty() {
    } else if vr.is_empty() {
    } else {
        if r.is_empty() {
        } else {
            println!("{:?}", vr);
        }
    }
}

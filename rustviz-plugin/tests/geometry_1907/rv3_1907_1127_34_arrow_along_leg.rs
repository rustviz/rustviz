fn longest<'a>(a: &'a String, b: &'a String) -> &'a String {
    if a.len() > b.len() { a } else { b }
}

fn main() {
    let s = String::from("hey");
    let s2 = String::from("hey");
    let mut r = &s2;
    let n = s.len() as i32;
    let r2 = r;
    if r.is_empty() {
        let r3 = longest(&s, &s);
    } else if s.len() > 1 {
        println!("{}", r2);
        if n > 0 { return; }
    } else {
        if s2.len() > 1 {
            let c2 = move || s;
        }
    }
}

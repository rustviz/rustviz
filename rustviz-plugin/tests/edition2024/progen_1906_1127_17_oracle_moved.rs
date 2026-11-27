struct Pair {
    a: String,
    b: String,
}

fn id<T>(t: T) -> T {
    t
}

fn run(r: &String, m: &mut String, o: Option<String>) -> Option<usize> {
    let s = format!("{}!", m);
    let p = Pair { a: id(s), b: String::from("ab") };
    if o.is_some() {
        let v = vec![p.b, String::from("ab")];
    } else {
        let s2 = if r.is_empty() {
            String::from("rust")
        } else {
            r.to_uppercase()
        };
    }
    Some(0)
}

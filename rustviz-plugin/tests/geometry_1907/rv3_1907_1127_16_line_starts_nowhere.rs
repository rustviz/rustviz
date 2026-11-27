fn id<T>(t: T) -> T {
    t
}

fn run(mut s: String, r: &String, mut o: Option<String>, flag: bool) -> Option<usize> {
    let mut s2 = id(s);
    let s3 = s2;
    if o.is_some() {
        loop {
            s2 = String::from("ab");
            break;
        }
    }
    match r.as_str() {
        "hi" | "ab" => {
            let s4 = o?;
        }
        _ => {
        }
    }
    Some(0)
}

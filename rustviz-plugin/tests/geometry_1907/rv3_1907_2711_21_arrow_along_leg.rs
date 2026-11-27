fn maybe(s: String) -> Option<String> {
    Some(s)
}

fn main() {
    let s = String::from("hey");
    let o = Some(s);
    let Some(x) = maybe(String::from("x")) else {
        return;
    };
    if x.len() > 1 {
    } else if o.is_some() {
        if let Some(x3) = o { } else { println!("{}", x); }
    } else {
        let r2 = o.as_ref().unwrap();
    }
}

fn first<'a>(s: &'a String) -> &'a str {
    s.as_str()
}

fn run(mut v: Vec<String>) -> Option<usize> {
    let s = String::from("hi");
    if let Some(x) = v.first() {
    } else {
        let Some(x2) = v.first() else { return None; };
    }
    Some(0)
}

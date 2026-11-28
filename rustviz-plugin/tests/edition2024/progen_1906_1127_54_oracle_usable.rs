fn first<'a>(s: &'a String) -> &'a str {
    s.as_str()
}

fn main() {
    let v: Vec<String> = Vec::new();
    if let Some(x2) = v.get(0) {
        if let Some(x3) = v.first() && x2.is_empty() {
            drop(v);
        } else {
        }
    }
}

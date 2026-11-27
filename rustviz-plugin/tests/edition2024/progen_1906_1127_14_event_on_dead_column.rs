fn first<'a>(s: &'a String) -> &'a str {
    s.as_str()
}

fn main() {
    let v: Vec<String> = Vec::new();
    if let Some(x3) = v.first() {
        if let Some(x4) = v.first() && x3.is_empty() {
        } else {
        }
        drop(v);
    }
}

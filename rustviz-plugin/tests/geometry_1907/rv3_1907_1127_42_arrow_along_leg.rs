fn first<'a>(s: &'a String) -> &'a str {
    s.as_str()
}

fn main() {
    let v: Vec<String> = Vec::new();
    if let Some(x) = v.first() && v.len() > 1 && let Some(y) = v.last() {
        if y.is_empty() {
            println!("{:?}", v);
        } else {
        }
    } else {
    }
}

fn first<'a>(s: &'a String) -> &'a str {
    s.as_str()
}

fn run(mut s: String, v: Vec<String>) -> Option<usize> {
    let s3 = s.to_string();
    let r2 = &s3;
    if let Some(x2) = v.first() {
    } else {
        if let Some(x3) = v.first() && r2.is_empty() {
        } else {
        }
    }
    Some(0)
}

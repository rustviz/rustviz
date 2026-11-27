fn first<'a>(s: &'a String) -> &'a str {
    s.as_str()
}

fn run(mut v: Vec<String>) -> Option<usize> {
    let s2 = String::from("rust");
    if let Some(x) = v.first() && v.len() > 1 && let Some(y) = v.last() {
        let s3;
        if y.is_empty() {
            s3 = s2;
        }
    }
    Some(0)
}

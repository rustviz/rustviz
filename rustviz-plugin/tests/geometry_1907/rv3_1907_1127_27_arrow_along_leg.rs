fn run(mut s: String, mut o: Option<String>) -> Option<usize> {
    let m = &mut s;
    if let Some(x2) = &o && x2.len() > 1 {
    } else {
        let s3 = if o.is_some() {
            format!("{}!", m)
        } else {
            format!("{}!", s)
        };
    }
    Some(0)
}

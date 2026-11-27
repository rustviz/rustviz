fn main() {
    let o: Option<String> = None;
    let Some(x) = o else { return; };
    let mut b = Box::new(x.to_string());
    let m = b.as_mut();
    let s3;
    if m.len() > 2 { s3 = (*b).clone(); } else { s3 = m.clone(); }
}

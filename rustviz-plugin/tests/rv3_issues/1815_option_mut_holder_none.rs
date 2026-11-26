fn main() {
    let mut v: Vec<String> = Vec::new();
    let o = v.last_mut();
    if o.is_none() { println!("empty"); }
}

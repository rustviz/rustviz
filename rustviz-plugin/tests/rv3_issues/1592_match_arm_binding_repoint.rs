fn main() {
    let s2 = String::from("abc");
    let v: Vec<String> = Vec::new();
    match v.first() {
        Some(mut x) => {
            x = &s2;
        }
        None => {}
    }
}

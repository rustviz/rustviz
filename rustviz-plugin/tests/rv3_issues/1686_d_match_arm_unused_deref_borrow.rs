fn main() {
    let mut b = Box::new(String::from("abc"));
    let x = b.as_str();
    match b.as_str() {
        "hi" => {}
        _ => {
            if x.len() > 1 {
                let r = &*b;
            } else {
                let n = 0;
            }
        }
    }
}

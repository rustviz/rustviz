fn split(s: String) -> (String, i32) {
    (s, 1)
}

fn main() {
    let s = String::from("x");
    let s2 = String::from("rust");
    let s3 = String::from("rust");
    let tp = split(s2);
    if s.len() > 1 {
        if s3.len() > 1 {
            let (s4, n) = tp;
            if s4.len() > 1 {
                return;
            }
        }
    }
}

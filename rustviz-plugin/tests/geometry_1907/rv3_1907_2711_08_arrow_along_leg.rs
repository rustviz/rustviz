fn main() {
    let s = String::from("x");
    let mut s2 = String::from("rust");
    let s3 = String::from("abc");
    if s3.len() > 1 {
    } else if s2.len() > 1 {
        let s4;
        s4 = s2;
        if s.len() > 1 {
            return;
        }
    } else {
        let m2 = &mut s2;
    }
}

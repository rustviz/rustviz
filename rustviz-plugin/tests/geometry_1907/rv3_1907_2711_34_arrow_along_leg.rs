fn main() {
    let mut s = String::from("rust");
    let mut s2 = s.clone();
    let v: Vec<String> = Vec::new();
    let x = s2.as_str();
    if v.len() > 1 {
        let x2 = s.as_str();
    } else if x.len() > 1 {
    } else {
        std::mem::swap(&mut s, &mut s2);
    }
}

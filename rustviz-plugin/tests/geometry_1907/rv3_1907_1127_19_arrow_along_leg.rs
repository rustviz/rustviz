fn main() {
    let s = String::from("x");
    let s2 = s.to_string();
    let mut v: Vec<String> = Vec::new();
    let mut r = v.last().unwrap();
    let m = v.iter_mut().next().unwrap();
    if s2.len() > 1 {
        r = v.last().unwrap();
    } else if s2.len() > 1 {
        let m2 = &mut *m;
    } else {
    }
}

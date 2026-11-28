fn main() {
    let s = String::from("ab");
    let tp = (String::from("ab"));
    if s.len() > 1 {
        let r = &s;
        drop(tp);
        if r.is_empty() {
            let mut r2 = r;
            println!("{} {}", r2, s);
        } else {
        }
    }
}

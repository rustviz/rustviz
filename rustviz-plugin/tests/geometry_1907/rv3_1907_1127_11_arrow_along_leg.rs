fn main() {
    let s = String::from("rust");
    let o = Some(s);
    let s2;
    s2 = String::from("abc");
    let b = Box::new(String::from("hi"));
    let s3 = String::from("x");
    if let Some(x) = o {
        println!("{}", s3);
    } else {
        if b.len() > 2 {
            return;
        }
    }
}

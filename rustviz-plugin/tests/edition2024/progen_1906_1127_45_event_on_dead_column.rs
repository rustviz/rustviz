fn first<'a>(s: &'a String) -> &'a str {
    s.as_str()
}

fn main() {
    let mut v: Vec<String> = Vec::new();
    let m = v.last_mut().unwrap();
    let mut r = &*m;
    println!("{:?}", v);
    let m2 = v.iter_mut().next().unwrap();
    r = v.first().unwrap();
}

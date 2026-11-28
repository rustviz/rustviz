fn first<'a>(s: &'a String) -> &'a str {
    s.as_str()
}

fn main() {
    let s = String::from("ab");
    let s2 = format!("{}!", s);
    let b = Box::new(String::from("hey"));
    let n = s2.len();
    let s3 = 'blk: {
        if s.len() > 1 { break 'blk format!("{}!", s2); }
        String::from("ab")
    };
    let x = first(&s2);
    let mut v: Vec<String> = Vec::new();
    println!("{:?} {}", v, s3);
    v[0] = String::from("x");
    let c = || s.len();
}

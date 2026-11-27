fn main() {
    let s = String::from("hi");
    let o = Some(s.clone());
    let x2 = &s[..];
    if x2.len() > 1 && let Some(x3) = &o && x3.len() > 1 {
    } else {
        let s3 = 'blk: {
            if s.len() > 1 { }
        };
    }
    let s4;
    s4 = s;
}

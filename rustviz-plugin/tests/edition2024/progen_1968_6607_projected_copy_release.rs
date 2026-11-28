struct Holder<'a> {
    r: &'a String,
    n: i32,
}

fn holder<'a>(s: &'a String) -> Holder<'a> {
    Holder { r: s, n: 0 }
}

fn main() {
    let s2 = String::from("hi");
    let h = holder(&s2);
    let s3;
    s3 = String::from("hi");
    if s3.len() > 1 {
        let Holder { r: r, .. } = h;
    } else {
    }
}

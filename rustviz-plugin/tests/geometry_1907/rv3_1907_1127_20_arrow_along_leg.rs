struct Holder<'a> {
    r: &'a String,
    n: i32,
}

fn holder<'a>(s: &'a String) -> Holder<'a> {
    Holder { r: s, n: 0 }
}

fn main() {
    let s = String::from("hi");
    let o = Some(String::from("hey"));
    let n = s.len() as i32;
    let n2 = s.len();
    if n2 > 0 {
        let h = holder(o.as_ref().unwrap());
    } else {
        let s2 = if n > 0 {
            format!("{}!", n)
        } else {
            s.clone()
        };
    }
}

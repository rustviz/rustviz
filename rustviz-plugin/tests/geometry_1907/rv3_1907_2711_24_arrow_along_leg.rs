struct Holder<'a> {
    r: &'a String,
    n: i32,
}

fn id<T>(t: T) -> T {
    t
}

fn main() {
    let s = String::from("hey");
    let s2 = s.to_uppercase();
    let o = Some(String::from("hi"));
    let h = Holder { r: &s, n: 1 };
    let r = h.r;
    let s3 = if r.is_empty() { String::from("hi") } else { String::from("x") };
    let s4 = o.unwrap();
    if s2.len() > 1 {
        let r2 = h.r;
        let s5 = if s3.len() > 1 {
            s4
        } else {
            String::from("ab")
        };
    } else if r.is_empty() {
        if s2.len() > 1 { return; }
    } else {
        let r3 = &s4;
        let s6 = 'blk: {
            if r3.is_empty() { break 'blk id(s4); }
            String::from("x")
        };
    }
}

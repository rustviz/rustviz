struct Holder<'a> {
    r: &'a String,
    n: i32,
}

fn id<T>(t: T) -> T {
    t
}

fn main() {
    let s = String::from("rust");
    let tp = (id(s), 1);
    let (ref r, _) = tp;
    let n = r.len();
    if r.is_empty() {
        let s3;
        s3 = String::from("ab");
    } else {
        let h = Holder { r: r, n: 1 };
        if n > 0 {
        } else {
            let r3 = h.r;
        }
    }
}

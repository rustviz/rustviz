struct Holder<'a> {
    r: &'a String,
    n: i32,
}

fn downgrade(s: &mut String) -> &String {
    s
}

fn holder<'a>(s: &'a String) -> Holder<'a> {
    Holder { r: s, n: 0 }
}

fn main() {
    let mut s = String::from("x");
    let s2 = String::from("x");
    let v: Vec<String> = Vec::new();
    if s2.len() > 1 {
        match v.as_slice() {
            [] => {
            }
            [x] => {
            }
            [x1, rest @ ..] => {
            }
        }
    } else {
        let h = holder(downgrade(&mut s));
        let m = &mut s;
    }
}

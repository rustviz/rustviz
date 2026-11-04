use std::fmt::Write;

fn check(v: &Vec<i32>) {
    if v.is_empty() {
        panic!("{}", v.len());
    }
    assert_eq!(v.len(), 3);
}

fn main() {
    let v = vec![1, 2, 3];
    let mut out = String::new();
    write!(out, "{:?}", v).unwrap();
    let w = vec![1, 2, 3];
    assert_eq!(v, w);
    check(&v);
    let msg = String::from("boom");
    if out.is_empty() {
        panic!("{}", msg);
    }
}

fn main() {
    let v = vec![1, 2, 3];
    let r = &v;
    let n = r.into_iter().count();
    let m = v.len();
    println!("{} {}", n, m);
}

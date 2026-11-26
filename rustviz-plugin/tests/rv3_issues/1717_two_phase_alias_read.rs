fn main() {
    let mut v = vec![1usize];
    let r = &v;
    v.push(r.len());
}

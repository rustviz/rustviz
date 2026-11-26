fn main() {
    let mut v = vec![1];
    let r = &mut v;
    r.push(
        r.len()
    );
    r.clear();
}

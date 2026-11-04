fn bump(v: &mut Vec<i32>) {
    let mut it = v.iter_mut();
    let first = it.next().unwrap();
    *first += 10;
    v.push(0);
}
fn main() {
    let mut v = vec![1, 2];
    bump(&mut v);
}

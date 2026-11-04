fn main() {
    let mut v = vec![1, 2, 3];
    for x in v.iter_mut() {
        *x *= 2;
    }
    println!("{:?}", v);
}

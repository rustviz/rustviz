fn main() {
    let mut v = vec![1, 2, 3];
    let x = &mut v[0];
    *x += 1;
    println!("{:?}", v);
    let mut it = v.iter_mut();
    while let Some(y) = it.next() {
        *y += 1;
    }
    println!("{:?}", v);
}

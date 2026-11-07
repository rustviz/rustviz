fn main() {
    let mut v = vec![1, 2, 3];
    if let Some(y) = v.get(1) {
        println!("{}", y);
    }
    if let Some(x) = v.get_mut(0) {
        *x += 1;
    }
    let mut it = v.iter();
    while let Some(n) = it.next() {
        println!("{}", n);
    }
}

fn main() {
    let mut v = vec![1, 2, 3];
    let mut it = v.iter_mut();
    let first = it.next().unwrap();
    *first += 10;
    v.push(4);
    println!("{:?}", v);
}

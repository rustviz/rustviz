fn main() {
    let mut v = vec![1, 2, 3];
    let mut it = v.iter_mut();
    while let Some(x) = it.next() {
        *x += 1;
        println!("{}", x);
    }
    let third = v.get(2);
    if let Some(t) = third {
        println!("{}", t);
    }
    println!("{:?}", third);
}

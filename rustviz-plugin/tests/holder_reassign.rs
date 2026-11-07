fn main() {
    let a = vec![1, 2];
    let b = vec![3, 4];
    let mut it = a.iter();
    println!("{:?}", it.next());
    it = b.iter();
    println!("{:?}", it.next());
}

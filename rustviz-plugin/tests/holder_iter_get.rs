fn main() {
    let v = vec![1, 2, 3];
    let mut it = v.iter();
    let third = v.get(2);
    let x = it.next().unwrap();
    println!("{} {:?} {}", x, third, v.len());
}

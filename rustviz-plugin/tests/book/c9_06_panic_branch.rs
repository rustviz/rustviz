fn main() {
    let v = vec![1, 2, 3];
    if v.len() > 100 {
        panic!("too long");
    }
    println!("{}", v.len());
}

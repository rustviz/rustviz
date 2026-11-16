fn main() {
    let a = String::from("a");
    let b = String::from("b");
    let refs: Vec<&String> = vec![&a, &b];
    let f: Box<dyn Fn() -> usize + '_> = Box::new(|| b.len());
    let v = vec![1, 2, 3];
    let it = v.iter();
    println!("{} {} {}", refs.len(), f(), it.len());
}

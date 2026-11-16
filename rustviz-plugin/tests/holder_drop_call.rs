fn main() {
    let a = String::from("a");
    let v = vec![1, 2, 3];
    let refs: Vec<&String> = vec![&a];
    let it = v.iter();
    drop(refs);
    drop(it);
}

fn show(v: &Vec<i32>) {
    let it = v.iter();
    println!("{:?}", it);
}
fn main() {
    let v = vec![1];
    show(&v);
}

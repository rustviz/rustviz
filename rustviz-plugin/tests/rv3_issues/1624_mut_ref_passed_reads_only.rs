fn show(v: &mut Vec<i32>) { println!("{:?}", v); }
fn main() {
    let mut x = vec![1];
    let r = &mut x;
    show(r);
}

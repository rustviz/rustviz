fn f(v: &Vec<i32>) {
    let r = &v[0];
    println!("{}", r);
}
fn g(v: &mut Vec<i32>) {
    let r = &mut v[0];
    *r += 1;
}
fn main() {
    let mut v = vec![1];
    f(&v);
    g(&mut v);
}

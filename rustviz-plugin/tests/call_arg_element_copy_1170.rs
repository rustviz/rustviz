// #1170: an element passed by value (`show(v[0])`, `w.push(v[1])`) is a
// copy of the i32 element, not of the Vec: v isn't Copy and keeps all of
// its contents. Drawn like `let n = v[0];`.
fn show(n: i32) { println!("{}", n); }
fn main() {
    let v = vec![1, 2];
    show(v[0]);
    let mut w = vec![5];
    w.push(v[1]);
    w.insert(0, w[0]);
    println!("{:?}", w);
}

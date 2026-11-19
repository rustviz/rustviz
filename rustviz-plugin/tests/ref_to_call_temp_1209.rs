// #1209: `&mut <call>` borrows a temporary the call returns, which rustc
// keeps until the end of the block; v stays borrowed (shared, as `iter`'s
// loan is) until `it`'s last use. The lender is v, not the function `iter`.
fn shared() {
    let v = vec![1, 2, 3];
    let it = &mut v.iter();
    println!("{:?}", it.next());
    println!("{:?}", v);
}
fn mutable() {
    let mut w = vec![1, 2, 3];
    let im = &mut w.iter_mut();
    if let Some(x) = im.next() {
        *x += 1;
    }
    println!("{:?}", w);
}
fn shared_ref() {
    let s = String::from("hi");
    let r = &s.as_str();
    println!("{}", r);
    println!("{}", s);
}
fn main() {}

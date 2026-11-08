// A holder moved into a `for` loop: the loop owns the iterator until it
// ends, so `v` is still mutably borrowed through the whole body (using
// `v` where the comment is would be E0502 / E0499), and its borrow ends
// where the loop does (#722).
fn main() {
    let mut v = vec![1, 2];
    let it = v.iter_mut();
    for x in it {
        *x += 1;
        // v is still mutably borrowed here
    } // end of the for
    v.push(3);
    let w = vec![String::from("a")];
    let it2 = w.iter();
    for s in it2 {
        println!("{}", s);
        // w is still borrowed here
    } // end of the second for
    println!("{:?}", w);
}

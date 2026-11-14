// #870: a call that borrows v while r (a `for` binding, a holder) still
// borrows it gives back only its own borrow: v stays shared until the
// last of those ends.
fn main() {
    let v = vec![String::from("a"), String::from("b")];
    let r = &v;
    println!("{}", v.len());
    println!("{:?}", r);
    for a in &v {
        let n = v.len();
        println!("{} {}", a, n);
    }
    let it = v.iter();
    let k = v.len();
    for x in it {
        println!("{} {}", x, k);
    }
}

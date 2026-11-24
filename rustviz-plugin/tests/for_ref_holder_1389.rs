// #1389: `for r in refs` over a Vec of references moves refs into the
// loop's iterator; each r is one of the references refs held, so it
// borrows what they borrow (a, b), not refs. The iterator keeps the loans
// live until the loop ends, where r gives a and b back. r used to give
// its borrow back to refs, which the loop had moved.
fn main() {
    let mut a = String::from("a");
    let mut b = String::from("b");
    let refs = vec![&mut a, &mut b];
    for r in refs {
        r.push('!');
    }
    println!("{} {}", a, b);
    let c = String::from("c");
    let d = String::from("d");
    let shared = vec![&c, &d];
    for s in shared {
        println!("{}", s);
    }
    println!("{} {}", c, d);
    let mut e = String::from("e");
    let more = vec![&mut e];
    for m in more.into_iter() {
        m.push('?');
    }
    println!("{}", e);
}

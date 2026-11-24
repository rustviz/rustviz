// #1448: a value holding a borrow moved into a call (`map(self, f)`,
// `unwrap(self)`): its loan goes on in what the call returns. When a
// variable keeps that (c, e), it gives the lender back, and the holder
// draws no end of its own after it was moved.
fn adapter() {
    let v = vec![String::from("a")];
    let it2 = v.iter();
    let c = it2.map(|s| s).next().unwrap();
    println!("{} {:?}", c, v);
}

fn unwrap_option() {
    let mut v = vec![String::new()];
    let o = v.last_mut();
    let e = o.unwrap();
    e.push('y');
    println!("{:?}", v);
}

// What the call returns keeps none of it: the `&String` copied out of
// refs borrows a1, and refs's loan ends with the statement.
fn copied_out() {
    let a1 = String::from("a1");
    let refs = vec![&a1];
    let it = refs.iter();
    let r = refs.iter();
    let c = it.copied().next().unwrap();
    println!("{} {:?} {:?}", c, r, refs);
}

// Consumed by the call (`count(self)`): the borrow ends there.
fn consumed() {
    let v = vec![String::from("a")];
    let it3 = v.iter();
    let n = it3.count();
    println!("{} {:?}", n, v);
}

fn main() {
    adapter();
    unwrap_option();
    copied_out();
    consumed();
}

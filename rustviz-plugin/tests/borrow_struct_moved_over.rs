// #700: `hold = other;` moves a struct holding a reference over one that
// holds another. The old value's borrow of a ends at the write (#569),
// and hold.r then holds the reference other.r had (to b) until its last
// use: hold.r's column runs on from the write to the `println!`. Main
// drew nothing between them, so hold.r looked dead where it is read.
// (Which loan hold.r holds after the move is #637: main still draws b's
// borrow given back from other.r. This fixture pins only the column.)
struct H<'a> { r: &'a String }

fn main() {
    let mut a = String::from("a");
    let b = String::from("b");
    let mut hold = H { r: &a };
    let other = H { r: &b };
    hold = other;
    a.push('!');
    println!("{}", hold.r);
}

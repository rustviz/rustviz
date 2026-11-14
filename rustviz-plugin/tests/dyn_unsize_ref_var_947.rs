// #947: `&gd` / `&mut gw2` with gd / gw2 a reference variable, unsized
// to a trait object, derefs once and borrows gd / gw2 itself: the
// `dyn Trait` value is the reference gd / gw2 holds, a value. Not a
// borrow of what gd / gw2 points to.
use std::fmt::Debug;
use std::io::Write;

fn shared() {
    let k = String::from("k");
    let gd = &k;
    let dd: &dyn Debug = &gd;
    println!("{:?}", dd);
}

fn exclusive() {
    let mut w = Vec::<u8>::new();
    let mut gw2 = &mut w;
    let dw: &mut dyn Write = &mut gw2;
    dw.write_all(b"x").unwrap();
}

// For comparison: an owner unsized to a trait object is lent as a
// resource, as before.
fn owner() {
    let k2 = String::from("k2");
    let d2: &dyn Debug = &k2;
    println!("{:?}", d2);
}

fn main() {
    shared();
    exclusive();
    owner();
}

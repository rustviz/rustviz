// A let-chain condition that moves a ManuallyDrop and then fails leaves
// it maybe-moved (#281). A ManuallyDrop has no drop glue, so no drop flag:
// its scope end drops nothing either way (#1147).

use std::mem::ManuallyDrop;

fn take(_m: ManuallyDrop<String>) -> bool {
    true
}

fn in_chain(o: Option<i32>) {
    let mc = ManuallyDrop::new(String::from("mc"));
    if let Some(_) = o && take(mc) {
        println!("taken");
    }
}

fn main() {
    in_chain(Some(1));
}

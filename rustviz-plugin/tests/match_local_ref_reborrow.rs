// A local reference matched on (#372): `r` gives its borrow of `x`
// back only after `s`, which reborrows through `r`, gives its back.

fn main() {
    let x = Some(String::from("hi"));
    let r = &x;
    match r {
        Some(s) => {
            println!("{}", s);
        }
        None => {}
    }
}

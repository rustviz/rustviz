// #1432: a reference with several lenders given a new value while another
// reference still borrows its first lender: its old loan ends at its last
// use (line 12), the shared lender a stays borrowed by z, and the other
// lender b is given back there. b used to stay borrowed to the end, and r's
// borrow ended at the write (line 14).
fn main() {
    let a = String::from("a");
    let b = String::from("b");
    let v = String::from("v");
    let z = &a;
    let mut r = if v.len() > 1 { &a } else { &b };
    println!("{}", r);
    println!("{}", z);
    r = &v;
    println!("{}", r);
}

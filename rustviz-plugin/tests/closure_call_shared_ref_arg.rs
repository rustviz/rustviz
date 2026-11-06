// A `&` borrow written at a closure call: used to crash the same
// way as #210.

fn main() {
    let s = String::from("hi");
    let print = |t: &String| println!("{}", t);
    print(&s);
    println!("{}", s);
}

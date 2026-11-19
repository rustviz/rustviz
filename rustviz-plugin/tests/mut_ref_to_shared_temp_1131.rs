// `&mut &x` borrows x shared, into a temporary `&String` that r borrows
// mutably: x is only ever immutably borrowed, and r can write the
// temporary (`*r = &y`). RustViz ended x's borrow as a mutable one (#1131).
struct H<'a, 'b> { r: &'a mut &'b String }
fn main() {
    let x = String::from("hi");
    let y = String::from("yo");
    let r = &mut &x;
    println!("{}", r);
    *r = &y;
    println!("{}", r);
    let z = String::from("z");
    let h = H { r: &mut &z };
    println!("{}", h.r);
    println!("{} {}", x, z);
}

fn main() {
    let s = String::from("hi");
    let r = &s;
    let n = 1;
    match n {
        1 => println!("{}", r),
        _ => {}
    }
}

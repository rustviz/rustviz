fn main() {
    let mut s = String::from("hi");
    let r = &mut s;
    let c = true;
    if c {
        r.push('x');
        println!("a");
    } else {
        println!("b");
    }
    println!("{}", s);
}

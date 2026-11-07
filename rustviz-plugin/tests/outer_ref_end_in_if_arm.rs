fn main() {
    let mut s = String::from("hi");
    let r = &s;
    let c = true;
    if c {
        println!("{}", r);
        s.push('x');
    } else {
        s.push('y');
    }
}

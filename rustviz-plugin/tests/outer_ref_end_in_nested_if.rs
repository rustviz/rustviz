fn main() {
    let s = String::from("hi");
    let r = &s;
    let c = true;
    let d = false;
    if c {
        if d {
            println!("{}", r);
        } else {
            println!("d");
        }
    } else {
        println!("c");
    }
}

fn main() {
    let s = String::from("hi");
    let arr = [s, String::from("x")];
    loop {
        if true {
            continue;
        }
        break;
    }
    println!("{:?}", arr);
}

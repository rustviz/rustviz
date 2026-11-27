fn main() {
    let mut s = String::from("hi");
    let tp = (s, 1);
    for _i in 0..2 {
        s = String::from("hey");
    }
    println!("{}", tp.1);
    s = tp.0;
}

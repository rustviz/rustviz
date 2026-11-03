fn main() {
    let s = String::from("x");
    for i in 0..10 {
        if i == 3 {
            break;
        }
        println!("{} {}", i, s);
    }
}

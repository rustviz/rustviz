fn main() {
    let n = 4;
    match n {
        1..=5 => println!("small"),
        _ => println!("big"),
    }
}

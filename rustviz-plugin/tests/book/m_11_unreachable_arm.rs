fn main() {
    let n = 2;
    let s = match n % 2 {
        0 => String::from("even"),
        1 => String::from("odd"),
        _ => unreachable!(),
    };
    println!("{}", s);
}

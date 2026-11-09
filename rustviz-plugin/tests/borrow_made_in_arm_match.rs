fn main() {
    let y = 9;
    let z = 7;
    let x = 8;
    let mut c = &x;
    let n = 2;
    match n {
        0 => c = &y,
        1 => c = &z,
        _ => println!("{}", c),
    }
}

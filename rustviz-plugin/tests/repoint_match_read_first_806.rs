fn main() {
    let y = 9;
    let z = 7;
    let x = 8;
    let mut c = &x;
    let n = 2;
    match n {
        0 => println!("{}", c),
        1 => c = &y,
        _ => c = &z,
    }
}

fn main() {
    let y = 9;
    let z = 7;
    let w = 6;
    let x = 8;
    let mut c = &x;
    let n = 2;
    match n {
        0 => c = &y,
        1 => c = &z,
        2 => c = &w,
        _ => println!("{}", c),
    }
}

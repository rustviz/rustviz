fn main() {
    let y = 9;
    let z = 7;
    let x = 8;
    let mut c = &x;
    let n = std::env::args().count();
    if n == 0 {
        c = &y;
    } else if n == 1 {
        c = &z;
    } else {
        println!("{}", c);
    }
}

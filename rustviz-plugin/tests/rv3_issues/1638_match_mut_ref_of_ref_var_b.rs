fn main() {
    let mut n = 0;
    let r = &mut n;
    match &r {
        s => println!("{}", **s),
    }
    *r += 1;
}

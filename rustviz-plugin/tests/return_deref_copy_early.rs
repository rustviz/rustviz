fn h(b: bool) -> i32 {
    let x = 5;
    let r = &x;
    if b {
        return *r;
    }
    0
}

fn main() {
    let n = h(true);
    println!("{}", n);
}

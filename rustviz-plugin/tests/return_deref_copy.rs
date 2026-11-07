fn h() -> i32 {
    let x = 5;
    let r = &x;
    *r
}

fn main() {
    let n = h();
    println!("{}", n);
}

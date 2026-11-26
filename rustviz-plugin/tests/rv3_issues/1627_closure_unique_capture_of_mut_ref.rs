fn main() {
    let mut n = 0;
    let r = &mut n;
    let mut f = || {
        println!("{}", r);
        *r += 1;
    };
    f();
}

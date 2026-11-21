// #1203: a closure names a static, it doesn't capture it: calling the
// closure writes the static.
static mut COUNT: i32 = 0;
fn main() {
    let f = || unsafe { COUNT = 9; };
    f();
    let g = || unsafe { COUNT += 1; };
    g();
    let n = unsafe { COUNT };
    println!("{}", n);
}

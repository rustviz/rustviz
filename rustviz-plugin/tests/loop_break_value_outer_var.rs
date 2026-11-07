// The loop breaks with a value declared before it (#431): `break s`
// moves `s` into `r`, so `s` has nothing to drop at the end of `main`
// and `r` does. A `Copy` value (`break n * 2`) is just a copy.

fn main() {
    let s = String::from("s");
    let mut n = 0;
    let r = loop {
        n += 1;
        if n == 3 {
            break s;
        }
    };
    let doubled = loop {
        n += 1;
        if n == 6 {
            break n * 2;
        }
    };
    println!("{} {}", r, doubled);
}

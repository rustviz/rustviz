// #1869: each iteration creates a new other, before the conditional.
struct I<'a> { r: &'a String }
struct O<'a> { inner: I<'a>, n: i32 }

fn main() {
    for c in [false, true] {
        let s = String::from("s");
        let t = String::from("t");
        let other = I { r: &t };
        let mut o = O { inner: I { r: &s }, n: 1 };
        if c {
            o.inner = other;
        }
        println!("{}", o.n);
    }
}

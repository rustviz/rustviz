// One `FnOnce` call, three captures with three fates (#271): `a` is
// handed on to a function, `b` is only read so it's dropped with the
// closure at the end of the call, and `n` is Copy so the closure just
// holds a copy.

fn eat(_s: String) {}

fn main() {
    let a = String::from("a");
    let b = String::from("b");
    let n = 5;
    let f = move || {
        eat(a);
        println!("{} {}", b, n);
    };
    f();
}

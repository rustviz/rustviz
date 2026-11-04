// The value a `break` carries reaches its receiver when the loop is the
// right side of an assignment, a tuple pattern, or a call argument too
// (#452): `r` gets `k`, `a` and `b` get `x` and `y`, `take` gets `z`.

fn take(s: String) {}

fn main() {
    let mut r = String::from("old");
    r = loop {
        let k = String::from("k");
        break k;
    };
    let (a, b) = loop {
        let x = String::from("x");
        let y = String::from("y");
        break (x, y);
    };
    take(loop {
        let z = String::from("z");
        break z;
    });
    println!("{} {} {}", r, a, b);
}

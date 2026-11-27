// #1869: the switch for !c tests c, and the moved place can be a field.
struct I<'a> { r: &'a String }
struct O<'a> { inner: I<'a>, n: i32 }

fn negated(c: bool) {
    let s = String::from("s");
    let t = String::from("t");
    let other = I { r: &t };
    let mut o = O { inner: I { r: &s }, n: 1 };
    if !c {
        o.inner = other;
    }
    println!("negated {}", o.n);
}

fn partial(c: bool) {
    let s = String::from("s");
    let t = String::from("t");
    let other = O { inner: I { r: &t }, n: 2 };
    let mut o = O { inner: I { r: &s }, n: 1 };
    if c {
        o.inner = other.inner;
    }
    println!("partial {}", o.n);
}

fn short_circuit(c: bool, d: bool) {
    let s = String::from("s");
    let t = String::from("t");
    let combined = I { r: &t };
    let mut o = O { inner: I { r: &s }, n: 1 };
    if c && d {
        o.inner = combined;
    }
    println!("combined {}", o.n);
}

fn main() {
    negated(false);
    partial(true);
    short_circuit(false, true);
    short_circuit(true, false);
    short_circuit(true, true);
}

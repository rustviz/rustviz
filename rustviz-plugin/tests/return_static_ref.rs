// Returning a reference to (part of) a static (#1053): the caller gets a
// reference, nothing is copied, and the borrow never ends inside the
// function. The static lives for the whole program, so the reference
// can be `&'static`.
struct Pt { x: i32, y: i32 }
static PRIMES: [i32; 3] = [2, 3, 5];
static ODDS: [i32; 2] = [1, 3];
static ORIGIN: Pt = Pt { x: 0, y: 0 };

fn primes() -> &'static [i32; 3] {
    &PRIMES
}

fn first() -> &'static i32 {
    &PRIMES[0]
}

fn origin_x() -> &'static i32 {
    &ORIGIN.x
}

fn either(c: bool) -> &'static [i32] {
    if c { &PRIMES } else { &ODDS }
}

fn main() {
    println!("{:?} {} {} {:?} {}", primes(), first(), origin_x(), either(true), ORIGIN.y);
}

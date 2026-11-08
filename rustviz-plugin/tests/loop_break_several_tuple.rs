// Several value `break`s (#561) bound to a tuple pattern: `s` gets the
// String of whichever `break` ran, `n` its number.

fn main() {
    let c = std::env::args().count() > 1;
    let x = String::from("x");
    let y = String::from("y");
    let (s, n) = loop {
        if c { break (x, 1); } else { break (y, 2); }
    };
    println!("{} {}", s, n);
}

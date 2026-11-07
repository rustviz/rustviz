// Several `break`s that carry a value, bound to a tuple pattern (#561).
// Pairing each `break (x, 1)` / `break (y, 2)` with `(s, n)` in its
// branch crashed ("no layout column for the timeline of event 10").
// This fixture only pins that it renders: the values aren't paired with
// `s` and `n` yet, so `s` is shown without a value after the loop
// (known wrong, listed in README Limitations; #588 fixes it).

fn main() {
    let c = std::env::args().count() > 1;
    let x = String::from("x");
    let y = String::from("y");
    let (s, n) = loop {
        if c { break (x, 1); } else { break (y, 2); }
    };
    println!("{} {}", s, n);
}

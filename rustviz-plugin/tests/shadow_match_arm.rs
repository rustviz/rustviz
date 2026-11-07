// A match-arm binding `n` shadows the outer `n` (#173). The `n`s of
// the two `Some(n)` arms share one column (`n₂`): only one of them
// exists at a time.

fn main() {
    let n = 5;
    let opt = Some(3);
    match opt {
        Some(n) if n > 2 => println!("big {}", n),
        Some(n) => println!("small {}", n),
        None => println!("none {}", n),
    }
}

// A reference picked by `if` / `match` borrows a string literal only if
// every arm that yields a value is one (an arm that diverges, like
// `panic!()`, yields none). `a`, `b`, `d` get the literal wording (`d`
// is a copy of `b`); `m` mixes a literal with a call result and `n` a
// literal with a const's reference, so both keep the generic text.
const NAME: &str = "rv";

fn pick(x: &str) -> &str { x } // rustviz: hide

fn main() {
    let c = std::env::args().count() > 1;
    let a = if c { "a" } else { "b" };
    let b = match a.len() {
        0 => "zero",
        1 => "one",
        _ => panic!("long"),
    };
    let d = b;
    let m = if c { "x" } else { pick(a) };
    let n = if c { "x" } else { NAME };
    println!("{} {} {} {} {}", a, b, d, m, n);
}

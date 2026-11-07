// #281: `let (p, q) = pair` always matches (rustc warns about the
// irrefutable pattern), so every way into the else branch passes the
// move: `pair`'s two Strings have been moved into `p` and `q`, which
// are dropped as `p.len() > 3` fails.

fn main() {
    let pair = (String::from("a"), String::from("b"));
    if let (p, q) = pair && p.len() > 3 {
        println!("{} {}", p, q);
    } else {
        println!("short");
    }
}

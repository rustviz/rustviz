// Several `break`s that carry a value (#561): only the `break` that
// ended the loop ran, so `r` owns whichever String it carried and drops
// it at the end of `main`. `a` and `b` may each have been moved into
// `r` (exactly one was): at the end of `main` the drop flag decides
// which of them still owns its String and drops it.

fn main() {
    let a = String::from("a");
    let b = String::from("b");
    let c = std::env::args().count() > 1;
    let r = loop {
        if c { break a; } else { break b; }
    };
    println!("{}", r);
}

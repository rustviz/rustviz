// `if let Msg::Pair(n, s) = msg`: `n` is copied out of `msg`, `s` is
// moved out. The copy changes nothing about `msg`, so it's drawn
// before the split; the move happens only if the pattern matches, so
// `msg`'s column still branches for it (#496).

enum Msg {
    Pair(i32, String),
    Quit,
}

fn take(_k: i32) {}
fn eat(_s: String) {}

fn main() {
    let msg = Msg::Pair(1, String::from("a"));
    if let Msg::Pair(n, s) = msg {
        take(n);
        eat(s);
    } else {
        take(0);
    }
    let _ = Msg::Quit;
}

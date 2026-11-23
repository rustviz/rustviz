// #1316, #1323: arrowheads and logos that still lay on another arrow after
// the arrow tracks of #1300. Each arrow or logo that does is drawn another
// way (another track, another angle into its dot, its dot's other side)
// where that leaves it on nothing, crossing no dot and adding no row.
enum Msg {
    Pair(i32, String),
}

// A one-line function whose parameter is moved into a call that it
// returns: the call's logo and the result's L to the caller.
fn wrap(s: String) -> Box<String> { Box::new(s) }

// The pattern's copy out of the raised (split) dot of msg arrives at n,
// which the move of msg into s hops over.
fn copy_and_move_out() {
    let msg = Msg::Pair(1, String::from("a"));
    let t = String::from("t");
    if let Msg::Pair(n, s) = msg { println!("{} {} {}", n, s, t); }
}

// #1323: a one-line `match` / `if let` returning to the caller, with a
// second pattern binding whose arrow ran along the arms' returns.
fn pick<'a>(o: Option<(&'a str, &'a str)>, d: &'a str) -> &'a str {
    match o { Some((x, _y)) => x, None => d }
}

fn first(o: Option<(String, String)>, d: String) -> String {
    if let Some((x, _y)) = o { x } else { d }
}

// Two calls' results moved into w at the loop's end, from two arms.
fn break_two_calls(n: usize, a: String) {
    let w = loop {
        match n { 0 => break a, 1 => break String::from("one"), _ => break String::from("many") }
    };
    println!("{}", w);
}

fn main() {
    println!("{}", wrap(String::from("a")));
    copy_and_move_out();
    println!("{}", pick(None, "d"));
    println!("{}", first(None, String::from("d")));
    break_two_calls(1, String::from("a"));
}

// #1187: a conditional written on one line whose arms each move two
// values. Each arm's second arrow runs on a row inserted below the arm,
// and the next arm, and the join, used to be drawn on those rows: an
// arrow of the `if` arm ran along the `else` arm's row, through its
// dots and along its arrows, and the `else` arm's along the join's row.
// Now each arm gets rows of its own, and the join a row below them.
fn two_and_two(c: bool, f: String, g: String, h: String, k: String) {
    let (a, b) = if c { (f, g) } else { (h, k) };
    println!("{} {}", a, b);
}

fn two_and_one(c: bool, f: String, g: String, h: String) {
    let (a, b) = if c { (f, g) } else { (h, String::new()) };
    println!("{} {}", a, b);
}

fn one_and_two(c: bool, f: String, h: String, k: String) {
    let (a, b) = if c { (f, String::new()) } else { (h, k) };
    println!("{} {}", a, b);
}

fn three_arms(n: u8, f: String, g: String, h: String, k: String, x: String, y: String) {
    let (a, b) = match n { 0 => (f, g), 1 => (h, k), _ => (x, y) };
    println!("{} {}", a, b);
}

fn swap(s: String, t: String) {
    let (x, y) = if s.len() > t.len() { (s, t) } else { (t, s) };
    println!("{} {}", x, y);
}

fn main() {
    two_and_two(true, String::new(), String::new(), String::new(), String::new());
    two_and_one(true, String::new(), String::new(), String::new());
    one_and_two(true, String::new(), String::new(), String::new());
    three_arms(1, String::new(), String::new(), String::new(), String::new(), String::new(), String::new());
    swap(String::new(), String::new());
}

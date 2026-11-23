// #1264: a conditional written on one line whose arms each draw one
// arrow. Its arms sit side by side on one row (#117), so when the two
// arrows come from the same side (f and h both left of a), f -> a runs
// past h's column alongside h -> a, and only one of the two could be
// hovered along that stretch. Now such arms get rows of their own, as
// arms that stack arrows do (#1187); arms whose arrows don't overlap keep
// their one-row diamond.
fn moves(c: bool, f: String, h: String) {
    let a = if c { f } else { h };
    println!("{}", a);
}

fn borrows(c: bool, f: String, h: String) {
    let r = if c { &f } else { &h };
    println!("{}", r);
}

fn mut_borrows(c: bool, mut f: String, mut h: String) {
    let r = if c { &mut f } else { &mut h };
    r.push('!');
}

fn copies(c: bool, x: i32, y: i32) {
    let a = if c { x } else { y };
    println!("{} {} {}", a, x, y);
}

fn else_if(c: bool, d: bool, f: String, g: String, h: String) {
    let a = if c { f } else if d { g } else { h };
    println!("{}", a);
}

fn match_arms(n: u8, f: String, g: String) {
    let r = match n { 0 => &f, _ => &g };
    println!("{}", r);
}

fn leftward(c: bool) {
    let mut a = String::new();
    println!("{}", a);
    let f = String::from("f");
    let h = String::from("h");
    a = if c { f } else { h };
    println!("{}", a);
}

fn different_targets(c: bool, f: String, h: String) {
    let mut a = String::new();
    let mut b = String::new();
    if c { b = f } else { a = h }
    println!("{} {}", a, b);
}

// No overlap: f -> a from the left, h -> a from the right.
fn opposite_sides(c: bool, f: String, mut a: String, h: String) {
    println!("{}", a);
    a = if c { f } else { h };
    println!("{}", a);
}

// No overlap: f -> a, then h -> b further right.
fn apart(c: bool, f: String, mut a: String, h: String, mut b: String) {
    println!("{} {}", a, b);
    if c { a = f } else { b = h }
    println!("{} {}", a, b);
}

// No overlap: a move into a call is a short arrow beside its dot.
fn take(s: String) {
    println!("{}", s);
}
fn into_calls(c: bool, f: String, h: String) {
    if c { take(f) } else { take(h) }
}

fn main() {
    moves(true, String::new(), String::new());
    borrows(true, String::new(), String::new());
    mut_borrows(true, String::new(), String::new());
    copies(true, 1, 2);
    else_if(true, false, String::new(), String::new(), String::new());
    match_arms(1, String::new(), String::new());
    leftward(true);
    different_targets(true, String::new(), String::new());
    opposite_sides(true, String::new(), String::new(), String::new());
    apart(true, String::new(), String::new(), String::new(), String::new());
    into_calls(true, String::new(), String::new());
}

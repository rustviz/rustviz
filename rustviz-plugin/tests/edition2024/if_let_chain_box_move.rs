// #579: `take(*b)` moves the String out of the Box `b`. It runs only
// when `y` matched, and the else branch also runs when `take` returns
// false, so there `*b` may have been moved.

fn take(s: String) -> bool { s.len() > 3 }

fn main() {
    let b = Box::new(String::from("a"));
    let y = Some(1);
    if let Some(_n) = y && take(*b) {
        println!("long");
    } else {
        println!("short or none");
    }
}

// #603: `*b = a` replaces the String in the Box (dropping the old one)
// before `b.len() > 3` is checked; the else branch sees either String.

fn main() {
    let y = Some(String::from("new"));
    let mut b = Box::new(String::from("old"));
    if let Some(a) = y && { *b = a; b.len() > 3 } {
        println!("long");
    } else {
        println!("short {}", b);
    }
}

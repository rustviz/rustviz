// #510: two pattern bindings on an `else if let` chain line. Each
// binding's column comes right after the one it borrows from (x, a, y,
// b), so both arrows run straight along the line, with no row added
// below it.

fn show(_r: &String) {}

fn main() {
    let x = Some(String::from("a"));
    let y = Some(String::from("b"));
    let c = true;
    if c {
        println!("c");
    } else if let Some(a) = &x && let Some(b) = &y {
        show(a);
        show(b);
    }
}

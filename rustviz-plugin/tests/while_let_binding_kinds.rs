// #382: `while let` patterns that borrow, copy and move. A loop body is
// drawn inline (no split), so each binding is drawn on the `while let`
// row, the row its column starts on.
fn show(_r: &String) {}
fn take(_k: i32) {}
fn consume(_o: String) {}

fn main() {
    let x = Some(String::from("a"));
    while let Some(a) = &x {
        show(a);
        break;
    }
    let mut y = Some(2);
    while let Some(b) = y {
        take(b);
        y = None;
    }
    let mut o = Some(String::from("c"));
    while let Some(s) = o {
        consume(s);
        o = None;
    }
}

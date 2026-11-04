// Three arms, the middle one holding an if/else: the middle arm sits on
// the column and the outer arms clear its nested branches (#119).
fn consume(_s: String) {}
fn show(_s: &String) {}

fn main() {
    let s = String::from("hi");
    let n = 1;
    let b = true;
    match n {
        0 => consume(s),
        1 => {
            if b {
                consume(s);
            } else {
                show(&s);
            }
        }
        _ => show(&s),
    }
}

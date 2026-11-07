// #604: in a let chain (edition 2024), a `let` scrutinee's temporary
// lives until the block the chain matched ends, or until the `else`
// runs when the chain fails; in a `while` let chain, until the end of
// the iteration. A temporary in a plain `&&` operand
// (`label().len() > 0`) is still dropped at the end of that operand.
fn make() -> Option<String> {
    Some(String::from("m"))
}

fn label() -> String {
    String::from("l")
}

fn main() {
    let n = 1;
    if n > 0 && let Some(_) = make() {
        println!("then");
    }
    if label().len() > 0 && let Some(_) = make() && n > 1 {
        println!("then");
    } else {
        println!("else");
    }
    let mut i = 0;
    while i < 2 && let Some(_) = make() {
        i += 1;
    }
}

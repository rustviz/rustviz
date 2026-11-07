// #605 (lifting #616's skip, now that #604 is fixed): the scrutinee
// temporary of a `let` in a let chain (edition 2024) whose pattern moves
// parts of it out. What is left (a refused `Err(String)`, the `_` String)
// is dropped with it: before the `else` runs, or when the block the chain
// matched ends; in a `while`, at the end of each iteration.
fn res() -> Result<String, String> {
    Ok(String::from("r"))
}

fn pair() -> (String, String) {
    (String::from("a"), String::from("b"))
}

fn main() {
    let n = 1;
    if n > 0 && let Ok(u) = res() {
        println!("{}", u);
    } else {
        println!("else");
    }
    if let (a, _) = pair() && a.len() > 3 {
        println!("{}", a);
    }
    let mut i = 0;
    while i < 2 && let Ok(w) = res() {
        i += 1;
        println!("{}", w);
    }
}

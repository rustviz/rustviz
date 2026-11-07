// #469: the scrutinee temporary of a `match` / `if let` / `while let`
// whose pattern binds parts of it by value. What the pattern leaves in
// it is dropped with it (edition 2024 timing).
fn make() -> Option<String> {
    Some(String::from("m"))
}

fn res() -> Result<String, String> {
    Ok(String::from("r"))
}

fn pair() -> (String, String) {
    (String::from("a"), String::from("b"))
}

fn main() {
    match pair() {
        (a, _) => println!("{}", a),
    }
    match make() {
        Some(s) if s.len() > 3 => println!("{}", s),
        _ => {}
    }
    if let Ok(u) = res() {
        println!("{}", u);
    } else {
        println!("none");
    }
    let mut k = 0;
    while let Ok(w) = res() {
        k += 1;
        if k > 2 {
            break;
        }
        println!("{}", w);
    }
    // Some(t) takes all of the String and None holds nothing, so
    // nothing that needs dropping is left: no temporary column.
    match make() {
        Some(t) => println!("{}", t),
        None => {}
    }
}

// #1167: a match guard is a temporary scope of its own. The String that
// `to_lowercase()` returns in a guard is dropped once the guard has been
// evaluated, before the arm's body (or the next arm) runs, whether the
// guard was true or false, wherever the `match` is.
fn tail() {
    let cmd = Some(String::from("Quit"));
    match cmd {
        Some(c) if c.to_lowercase() == "quit" => println!("bye"),
        Some(c) => println!("{}", c),
        None => {}
    }
}

fn statement(o: Option<i32>) -> i32 {
    match o {
        Some(x) if x.to_string().len() > 1 => {
            println!("big");
        }
        _ => {}
    }
    1
}

fn value(o: Option<String>) -> usize {
    let n = match o {
        Some(ref s) if s.to_uppercase().starts_with('A') => 1,
        Some(_) => 2,
        None => 0,
    };
    n
}

fn main() {
    tail();
    println!("{}", statement(Some(10)));
    println!("{}", value(Some(String::from("a"))));
}

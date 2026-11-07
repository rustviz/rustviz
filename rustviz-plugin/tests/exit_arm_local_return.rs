// A match arm that returns drops its pattern's binding `s` and its own
// `y` at the `return`, with `t` (#630), not at the arm's closing brace.

fn f(o: Option<String>) -> usize {
    let t = String::from("t");
    match o {
        Some(s) => {
            let y = String::from("y");
            return s.len() + y.len() + t.len();
        }
        None => {}
    }
    t.len()
}

fn main() {
    println!("{}", f(Some(String::from("s"))));
}

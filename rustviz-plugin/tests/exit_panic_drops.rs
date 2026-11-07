// A panic unwinds the stack, dropping every value the function still
// owns, right at the `panic!` (#410). `s` is only borrowed by the
// panic's message, so it's dropped there too. `std::process::exit`
// also never returns, but runs no destructors: nothing is dropped at
// it, and its arm's columns end in a bar.

fn show(s: &String) -> usize {
    s.len()
}

fn check(s: String, n: i32) -> usize {
    let t = String::from("t");
    match n {
        0 => panic!("bad {}", s),
        1 => todo!(),
        2 => std::process::exit(1),
        _ => {
            show(&s);
        }
    }
    t.len()
}

fn main() {
    println!("{}", check(String::from("s"), 3));
}

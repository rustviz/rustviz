// #841: `s` is not `mut`, so it can never be assigned again (E0384); the
// join dot must not say "(until it's assigned again)".
fn main() {
    let s = String::from("hello");
    let keep = false;
    if keep {
        println!("{}", s);
    } else {
        takes_ownership(s);
    }
}

fn takes_ownership(some_string: String) {
    println!("{}", some_string);
}

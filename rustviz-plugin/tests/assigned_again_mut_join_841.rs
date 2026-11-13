// #841: a `let mut` binding can be assigned again, so the join dot keeps
// the "(until it's assigned again)" clause.
fn main() {
    let mut s = String::from("hello");
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

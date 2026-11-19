// #1144: a slice pattern in a `match` arm / `if let` binds the elements
// it names, moving them out of the array. #1166: elements a slice
// pattern doesn't take (`_`, `..`) stay in the array and are dropped
// with it.
fn match_all() {
    let arr = [String::from("a"), String::from("b")];
    match arr {
        [a, b] => println!("{} {}", a, b),
    }
}
fn match_some() {
    let pair = [String::from("a"), String::from("b")];
    match pair {
        [c, _] => {
            println!("{}", c);
        }
    }
}
fn if_let_ends() {
    let trio = [String::from("a"), String::from("b"), String::from("c")];
    if let [first, .., last] = trio {
        println!("{} {}", first, last);
    }
}
fn let_some() {
    let duo = [String::from("a"), String::from("b")];
    let [d, _] = duo;
    println!("{}", d);
}
fn let_rest() {
    let three = [String::from("a"), String::from("b"), String::from("c")];
    let [e, ..] = three;
    println!("{}", e);
}
fn copies() {
    let nums = [1, 2, 3];
    match nums {
        [x, y, z] => println!("{} {} {}", x, y, z),
    }
}
fn main() {
    match_all();
    match_some();
    if_let_ends();
    let_some();
    let_rest();
    copies();
}

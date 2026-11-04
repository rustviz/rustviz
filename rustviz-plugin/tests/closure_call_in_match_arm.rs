// A closure called in one arm of a match (#133): the call's dots go
// in that arm's column, on the call's own line.

fn main() {
    let s = String::from("hi");
    let f = || s.len();
    let n = match s.is_empty() {
        true => 0,
        false => f(),
    };
    println!("{}", n);
}

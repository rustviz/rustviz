// A `&mut` borrow written at a closure call (#210, used to crash):
// `s` is lent to the call for its duration, like an argument to a
// named function.

fn main() {
    let mut s = String::from("a");
    let mut push = |q: &mut String| q.push('b');
    push(&mut s);
    println!("{}", s);
}

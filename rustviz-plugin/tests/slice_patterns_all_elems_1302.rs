// #1302: slice patterns on several lines that together take every element
// of an array. Each takes only its elements; the array is empty once the
// last of them has run, and its scope end drops nothing. main drew each
// as a move of the whole array, and the second went `State::Invalid`.
// In `match` arms that take different elements, no one arm empties it:
// each element is dropped at the scope end only if no arm moved it.
fn two() {
    let arr = [String::from("a"), String::from("b")];
    let [a, _] = arr;
    let [_, b] = arr;
    println!("{} {}", a, b);
}
fn three() {
    let tri = [String::from("a"), String::from("b"), String::from("c")];
    let [a, _, _] = tri;
    let [_, b, c] = tri;
    println!("{} {} {}", a, b, c);
}
fn rest() {
    let rs = [String::from("a"), String::from("b"), String::from("c")];
    let [a, ..] = rs;
    let [_, tail @ ..] = rs;
    println!("{} {}", a, tail.len());
}
fn steps() {
    let st = [String::from("a"), String::from("b"), String::from("c")];
    let [a, _, _] = st;
    let [_, b, _] = st;
    let [_, _, c] = st;
    println!("{} {} {}", a, b, c);
}
fn in_branch(c: bool) {
    let br = [String::from("a"), String::from("b")];
    let [a, _] = br;
    if c {
        let [_, b] = br;
        println!("{}", b);
    }
    println!("{}", a);
}
fn arms(c: bool) {
    let am = [String::from("a"), String::from("b")];
    match am {
        [a, _] if c => println!("{}", a),
        [_, b] => println!("{}", b),
    }
}
fn param(pa: [String; 2]) {
    let [a, _] = pa;
    let [_, b] = pa;
    println!("{} {}", a, b);
}
fn main() {
    two(); three(); rest(); steps(); in_branch(true); arms(false);
    param([String::from("a"), String::from("b")]);
}

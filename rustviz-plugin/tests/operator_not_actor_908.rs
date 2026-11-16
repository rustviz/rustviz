// #908: an operator is never an actor. A built-in operator on Copy
// values (`item == b' '`, `x + y`, `-x`, `acc += x`) copies its
// operands, drawn on their own columns; it gets no column and no
// arrow. `==` on Strings borrows its operands for the call
// (`PartialEq::eq(&a, &b)`); `s1 + &s2` is `Add::add(s1, &s2)`, a real
// call that moves s1.
fn first_word(s: &String) -> usize {
    let bytes = s.as_bytes();
    for (i, &item) in bytes.iter().enumerate() {
        if item == b' ' {
            return i;
        }
    }
    s.len()
}

fn main() {
    let x = 5;
    let y = 3;
    let b = true;
    let lt = x < y;
    let sum = x + y;
    let nx = -x;
    let nb = !b;
    let mut acc = 0;
    acc += y;
    let s1 = String::from("tic");
    let s2 = String::from("tac");
    let same = s1 == s2;
    let s3 = s1 + &s2;
    let n = first_word(&s3);
    println!("{} {} {} {} {} {} {} {}", lt, sum, nx, nb, acc, same, s3, n);
}

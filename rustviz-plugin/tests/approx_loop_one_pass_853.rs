// #853: a loop is drawn as one pass through its body. The first dot of
// each column inside the body says so; a variable the loop declares is
// named ("each pass has its own s"). Dots outside the loop, on the loop's
// first line (where a `for` loop's iterator is made) and on its closing
// line, get no note, nor does `n`, bound once by the `break`, nor a
// variable of an `if` block.
fn main() {
    let mut total = 0;
    let words = vec![String::from("a"), String::from("b")];
    for w in words.iter() {
        let s = w.clone();
        total += s.len();
    }
    let mut stack = vec![1, 2];
    while let Some(top) = stack.pop() {
        total += top;
    }
    let n = loop {
        let t = String::from("t");
        break t.len();
    };
    let after = total + n;
    if after > 0 {
        let msg = String::from("big");
        println!("{}", msg);
    }
    println!("{} {}", after, words.len());
}

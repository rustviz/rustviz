// #853: a loop is drawn as one pass through its body. Each loop gets one
// note saying so, on its first dot drawn for a pass: `w` on the `for`
// line, `stack`'s `pop` in the `while let` condition (checked on every
// pass), `t` in the `loop`. No other dot gets one: not those outside the
// loops, not `words` lent to the iterator on the `for` line (once) or
// given back on the closing line, not `n`, bound once by the `break`,
// and not a variable of an `if` block.
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

// #868: a `for` over an iterator that a call made from a variable keeps
// that variable borrowed until the loop ends: `next()` runs on the
// iterator at the top of every iteration, so `names.push(..)` anywhere in
// the body is E0502. The borrow used to end on the `for` line.
fn main() {
    let names = vec![String::from("ann"), String::from("bo")];
    for (i, name) in names.iter().enumerate() {
        println!("{} {}", i, name);
    }
    let nums = vec![1, 2, 3];
    for n in nums.iter().map(|x| x + 1) {
        println!("{}", n);
    }
    let words = vec![String::from("x"), String::from("y")];
    for w in words.iter().rev() {
        println!("{}", w);
    }
    let mut tags = vec![String::from("a")];
    for (i, t) in tags.iter_mut().enumerate() {
        t.push('!');
        println!("{}", i);
    }
    let s = String::from("hi");
    for c in s.chars() {
        println!("{}", c);
    }
    println!("{} {} {} {} {}", names.len(), nums.len(), words.len(), tags.len(), s.len());
}

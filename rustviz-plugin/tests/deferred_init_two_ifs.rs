// Two `if`s without `else` each assign s (#244). After the first, s
// holds a String only if a was true; the second's assignment drops it
// only then, and on its implicit `else` path s still may have no value.
// The last assignment drops whichever String s holds, if any.

fn main() {
    let a = true;
    let b = false;
    let mut s;
    if a {
        s = String::from("a");
        println!("{}", s);
    }
    if b {
        s = String::from("b");
        println!("{}", s);
    }
    s = String::from("c");
    println!("{}", s);
}

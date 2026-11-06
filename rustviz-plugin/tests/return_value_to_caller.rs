// A `return` hands its value to the caller, like a tail expression:
// `s` is moved there, `n` is copied there (#515).
fn pick(flag: bool, s: String) -> String {
    if flag {
        return s;
    }
    String::from("other")
}

fn count(v: &Vec<i32>) -> i32 {
    let n = v.len() as i32;
    if n > 2 {
        return n;
    }
    0
}

fn main() {
    let s = String::from("mine");
    let r = pick(true, s);
    println!("{}", r);
    println!("{}", count(&vec![1, 2, 3]));
}

// #776: an `if let` / let-chain scrutinee's temporary, edition 2024.
// With no `else` there is no `else` block to name: the temporary drops
// when the pattern doesn't match (before the code after the `if`), or
// when the matched block ends. With an `else`, the old wording stays.
fn names() -> Vec<String> {
    vec![String::from("a")]
}

fn boxed() -> Option<Box<i32>> {
    Some(Box::new(1))
}

fn main() {
    if let Some(_) = names().pop() {
        println!("matched");
    }
    let ok = true;
    if ok && let Some(_) = boxed() {
        println!("chain matched");
    }
    if let Some(_) = Some(String::from("e")) {
        println!("a");
    } else {
        println!("b");
    }
    if ok && let Some(_) = Some(Box::new(2.5)) {
        println!("c");
    } else {
        println!("d");
    }
    println!("after");
}

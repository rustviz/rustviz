// The chain's binding `a` shadows a variable `a` that is still in scope:
// it is a new variable (`a₂`), not an assignment to the outer `a`. When
// `n > 0` is false, `a₂` goes out of scope with the String it took from
// `p`. That drop wasn't stated: the chain looked up which of its
// bindings need drop by identifier (`a`), not by column (`a₂`).

pub fn f(n: i32, p: Option<String>) {
    let a = String::from("x");
    if let Some(a) = p && n > 0 {
        println!("{a}");
    } else {
        println!("no");
    }
    println!("{a}");
}

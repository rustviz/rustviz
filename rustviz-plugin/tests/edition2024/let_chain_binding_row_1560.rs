// #1560's rule in a let-chain: x's borrow from o leaves o's arm dot, where
// o's solid fork leg comes in at 11 degrees; the line gets a row of its own
// and the arrow a level track below the fork.

fn id<T>(t: T) -> T { t }
fn let_chain_binding() {
    let s = String::from("hey");
    let s2 = id(s);
    let o = Some(s2);
    let n = 3;
    if n > 0 && let Some(x) = &o && x.len() > 1 {
        let s4 = if x.is_empty() { o.unwrap() } else { format!("{}!", n) };
    }
}


fn store_false<'a>(v: &mut Vec<&'a String>, s: &'a String) -> bool {
    v.push(s);
    false
}

// The let-chain's call stores &t in refs and is false: the borrow lives
// on into the else branch.
fn setter_in_condition(o: Option<u8>) {
    let a = String::from("a");
    let t = String::from("t");
    let mut refs = vec![&a];
    if let Some(_n) = o && store_false(&mut refs, &t) {
        println!("stored");
    } else {
        println!("{t}");
    }
    println!("{}", refs.len());
}

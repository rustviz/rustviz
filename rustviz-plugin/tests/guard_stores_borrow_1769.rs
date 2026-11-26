fn store_false<'a>(v: &mut Vec<&'a String>, s: &'a String) -> bool {
    v.push(s);
    false
}

// The guard's call stores &guard_t in guard_refs and is false: the
// borrow lives on into the later arm.
fn setter_in_guard(k: u8) {
    let a = String::from("a");
    let guard_t = String::from("t");
    let mut guard_refs = vec![&a];
    match k {
        0 if store_false(&mut guard_refs, &guard_t) => {}
        _ => {
            println!("{guard_t}");
        }
    }
    println!("{}", guard_refs.len());
}

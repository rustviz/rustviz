// #1391: `*b = ..` on a Box inside a condition (a plain `if`, a let
// chain, a match guard, a `while let` chain) drops the old String: the
// write's dot has the drop mark, as `*b = ..;` outside a condition does
// (#1345). In a condition the drop wasn't drawn, or was drawn on a dot of
// its own after the condition's later events.
fn main() {
    let mut b = Box::new(String::from("old"));
    if { *b = String::from("x"); b.len() > 3 } {
        println!("long");
    }
    let o = Some(3);
    match o {
        Some(n) if { *b = String::from("g"); n > 1 } => println!("{}", b),
        _ => {}
    }
    let mut k = 0;
    while let Some(_) = o && { *b = String::from("w"); k < 2 } {
        k += 1;
    }
    let y = Some(String::from("new"));
    if let Some(a) = y && { *b = a; b.len() > 3 } {
        println!("{}", b);
    }
}

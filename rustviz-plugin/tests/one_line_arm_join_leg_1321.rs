// #1321: a one-line `if let` / `match` arm with rows of its own (#1264)
// whose borrow of s ends on the conditional's line (the `&mut s` k2
// holds, moved into x and lent to `push`). The arm's lent stretch runs
// down to the dot where s gets its value back; the leg into the join
// starts at that dot. Main drew the leg from the row above, a solid
// "s is the owner" diagonal starting in mid-air beside the lent stretch.
fn into_some() {
    let mut s = String::from("b");
    let r2 = &mut s;
    let k2 = Some(r2);
    if let Some(x) = k2 { x.push('!'); }
}
fn with_else() {
    let mut s = String::from("b");
    let r2 = &mut s;
    let k2 = Some(r2);
    if let Some(x) = k2 { x.push('!'); } else { println!("none"); }
    println!("{}", s);
}
fn match_one_line() {
    let mut s = String::from("b");
    let k2 = Some(&mut s);
    match k2 { Some(x) => x.push('!'), None => {} }
    println!("{}", s);
}
fn main() {}

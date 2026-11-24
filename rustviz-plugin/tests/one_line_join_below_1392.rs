// #1392: a one-line conditional whose arm stacks its arrows on rows
// below the line joins a row below the arm's last dot, not level with
// it (s's "no longer mutably borrowed" dot beside the join dot).
fn top_level() {
    let mut s = String::from("b");
    let r2 = &mut s;
    let k2 = Some(r2);
    if let Some(x) = k2 { x.push('!'); }
}
fn in_loop() {
    let mut s = String::from("b");
    let k2 = Some(&mut s);
    for _ in 0..1 {
        if let Some(x) = k2 { x.push('!'); }
        break;
    }
}
fn one_line_match() {
    let mut s = String::from("b");
    let k2 = Some(&mut s);
    match k2 { Some(x) => x.push('!'), None => {} }
    println!("{}", s);
}
fn with_else() {
    let mut s = String::from("b");
    let k2 = Some(&mut s);
    if let Some(x) = k2 { x.push('!'); } else { }
    println!("{}", s);
}
fn nested_in_arm(c: bool) {
    let mut s = String::from("b");
    let k2 = Some(&mut s);
    if c {
        if let Some(x) = k2 { x.push('!'); }
    }
    println!("{}", s);
}
fn adjacent() {
    let mut s = String::from("b");
    let mut t = String::from("c");
    let k2 = Some(&mut s);
    let k3 = Some(&mut t);
    if let Some(x) = k2 { x.push('!'); }
    if let Some(y) = k3 { y.push('!'); }
    println!("{} {}", s, t);
}
fn main() {}

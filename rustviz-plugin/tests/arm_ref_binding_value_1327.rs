// #1327: an arm binding that borrows the scrutinee (`match &o`, `ref x`)
// and is the arm's value: r holds that borrow of o after the `match`, so
// o stays borrowed until r's last use, not just to the arm's end, and r
// borrows o (never x, a reference r copies).
fn let_match_ref() {
    let o1: Option<String> = Some(String::from("hi"));
    let t1 = String::from("t");
    let r1 = match &o1 {
        Some(x1) => x1,
        None => &t1,
    };
    println!("{}", r1);
    println!("{:?}", o1);
}

fn ref_pattern() {
    let o2: Option<String> = Some(String::from("hi"));
    let t2 = String::from("t");
    let r2 = match o2 { Some(ref x2) => x2, None => &t2 };
    println!("{}", r2);
}

fn main() {
    let_match_ref();
    ref_pattern();
}

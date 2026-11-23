// #1327: `let r = match o { Some(x) => x, None => "d" };` crashed: r was
// declared before the arms were visited, when the arm's own binding x had
// no column yet. r is a copy of the reference x took out of o (or of the
// other arm's reference), so r borrows what o's Some held, or the literal.
struct H<'a> { r: &'a String }

fn literal_default() {
    let o1: Option<&str> = None;
    let r1 = match o1 { Some(x1) => x1, None => "d" };
    println!("{}", r1);
}

fn if_let_ref_default() {
    let s2 = String::from("hi");
    let d2 = &s2;
    let o2: Option<&String> = Some(&s2);
    let r2 = if let Some(x2) = o2 { x2 } else { d2 };
    println!("{}", r2);
}

fn mut_ref() {
    let mut s3 = String::from("hi");
    let mut t3 = String::from("t");
    let o3: Option<&mut String> = Some(&mut s3);
    let r3 = match o3 { Some(x3) => x3, None => &mut t3 };
    r3.push('!');
    println!("{}", r3);
}

fn nested() {
    let s4 = String::from("hi");
    let o4: Option<Option<&String>> = Some(Some(&s4));
    let t4 = String::from("t");
    let r4 = match o4 { Some(Some(x4)) => x4, Some(None) => &t4, None => &t4 };
    println!("{}", r4);
}

fn method_arm() {
    let s5 = String::from("hi");
    let o5: Option<&String> = Some(&s5);
    let r5 = match o5 { Some(x5) => x5.as_str(), None => "d" };
    println!("{}", r5);
}

fn branch_local(c: bool) {
    let a6 = String::from("a");
    let b6 = String::from("b");
    let r6 = if c { let y6 = &a6; y6 } else { &b6 };
    println!("{}", r6);
}

fn field_init() {
    let s7 = String::from("s");
    let t7 = String::from("t");
    let o7 = Some(&s7);
    let h7 = H { r: match o7 { Some(x7) => x7, None => &t7 } };
    println!("{}", h7.r);
}

fn pick<'a>(o8: Option<&'a str>, d8: &'a str) -> &'a str {
    let r8 = if let Some(x8) = o8 {
        x8
    } else {
        d8
    };
    r8
}

fn main() {
    literal_default();
    if_let_ref_default();
    mut_ref();
    nested();
    method_arm();
    branch_local(true);
    field_init();
    println!("{}", pick(None, "d"));
}

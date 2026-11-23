// #1369: `&gx` with gx a `&String`, coerced to `&String`, reborrows
// `*gx` (#939). rustc's first step borrows gx itself (`&**(&gx)`), and
// that loan may end at once; what the result borrows is what gx points
// to, so its borrow lasts to its own last use, as for the written
// `&*gx`. (`gx = &d;` while s is live compiles; `c.push('x')` doesn't.)
struct Holder<'a> {
    r: &'a String,
}

fn let_coerced() {
    let c1 = String::from("c");
    let gx1 = &c1;
    let s1: &String = &gx1;
    println!("{}", s1);
}

fn let_to_str() {
    let c2 = String::from("c");
    let gx2 = &c2;
    let s2: &str = &gx2;
    println!("{}", s2);
}

fn assigned() {
    let c3 = String::from("c");
    let gx3 = &c3;
    let s3: &String;
    s3 = &gx3;
    println!("{}", s3);
}

fn struct_field() {
    let c4 = String::from("c");
    let gx4 = &c4;
    let h4 = Holder { r: &gx4 };
    println!("{}", h4.r);
}

fn ctor_arg() {
    let c5 = String::from("c");
    let gx5 = &c5;
    let o5: Option<&String> = Some(&gx5);
    println!("{:?}", o5);
}

fn in_branch(b: bool) {
    let c6 = String::from("c");
    let d6 = String::from("d");
    let gx6 = &c6;
    let gy6 = &d6;
    let s6: &String = if b { &gx6 } else { &gy6 };
    println!("{}", s6);
}

fn pushed() {
    let c7 = String::from("c");
    let gx7 = &c7;
    let mut v7: Vec<&String> = Vec::new();
    v7.push(&gx7);
    println!("{:?}", v7);
}

// gx8 may point elsewhere while s8 is live: s8 borrows c8, not gx8.
fn repointed() {
    let c8 = String::from("c");
    let d8 = String::from("d");
    let mut gx8 = &c8;
    let s8: &String = &gx8;
    gx8 = &d8;
    println!("{} {}", s8, gx8);
}

fn main() {
    let_coerced();
    let_to_str();
    assigned();
    struct_field();
    ctor_arg();
    in_branch(true);
    pushed();
    repointed();
}

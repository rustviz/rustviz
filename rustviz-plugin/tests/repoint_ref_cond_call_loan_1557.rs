// #1557: a reference first set by a conditional whose condition borrows
// its lender too (`if a.len() > 0 { &a } else { &b }`, two loans of a on
// that line) and re-pointed later. r holds `&a`'s loan, not `a.len()`'s,
// which ends on line 5: main ended r's old loan of a there, on the `let`'s
// row. r is read on line 12 on the `_` path, so a stays borrowed there.
fn main() {
    let a = String::from("a");
    let b = String::from("b");
    let t = String::from("t");
    let mut r = if a.len() > 0 { &a } else { &b };
    let x = &b;
    match t.len() {
        0 => {
            r = &t;
            println!("{}", r);
        }
        _ => println!("{}", r),
    }
    println!("{}", x);
    in_if();
    no_else();
}

// A `match` first, re-pointed in an `if`.
fn in_if() {
    let a2 = String::from("a");
    let b2 = String::from("b");
    let t2 = String::from("t");
    let mut r2 = match a2.len() { 0 => &b2, _ => &a2 };
    if t2.len() > 0 {
        r2 = &t2;
        println!("{}", r2);
    } else {
        println!("{}", r2);
    }
    println!("{}", b2);
}

// Re-pointed on one path only: r3 still points to a3 on line 50.
fn no_else() {
    let a3 = String::from("a");
    let t3 = String::from("t");
    let mut r3 = if a3.is_empty() { &t3 } else { &a3 };
    if t3.len() > 3 {
        r3 = &t3;
    }
    println!("{}", r3);
}

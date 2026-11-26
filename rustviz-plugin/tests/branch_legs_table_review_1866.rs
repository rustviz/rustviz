// #1866's review: an arm's column reads the state table on the arm's own
// rows, and keeps 2.x's style where the table can't say the arm's path.
fn downgrade(s: &mut String) -> &String {
    s
}

// v is borrowed by `maybe` only when `choose` was true: in the `if e`
// arm it may still be shared, not surely.
fn maybe_shared(v: Vec<String>, choose: bool, e: bool) {
    let maybe = if choose { v.first() } else { None };
    if e {
        let n = v.len();
        println!("{}", n);
    } else {
        let k = 0;
        println!("{}", k);
    }
    maybe.unwrap();
}

// A comment or a string with `=>` in it doesn't make a row a match arm.
fn arrow_in_text() {
    let mut s2 = String::from("rust");
    let m = &mut s2;
    if m.len() > 2 {
        let n = m.len();
        let r3 = downgrade(&mut s2); /* => */
        println!("=> {}", n);
        let m2 = &mut s2;
        m2.push('a');
    } else {
        m.push('a');
    }
}

// The arm's blank row carries the row above: x is free after `z.len()`.
fn blank_row(c: bool) {
    let mut x = String::from("x");
    let z = &mut x;
    match c {
        true => {
            z.len();

            println!("{}", x);
        }
        false => {
            z.push('b');
        }
    }
}

// The dots stacked under `let _r2 = r;` keep 2.x's line: the table's
// state is the one after the whole line.
fn stacked() {
    let c = 1;
    let mut s = String::from("hi");
    let m = &mut s;
    let r = &*m;
    match c {
        0 => {
            let _r2 = r;
        }
        _ => {
            println!("{}", r);
        }
    }
}

// #1489: a dashed (inactive) fork leg is background: an arrow may cross it
// at any angle. Solid legs keep the strict check and #1560's row.

// s's borrow leaves its `if` arm's dot beside s's dashed `else` leg.
fn one_line_else_if() {
    let s = String::from("hi");
    let mut n = 0;
    if n > 0 { let x4 = s.as_str(); } else if s.len() > 1 { drop(s); } else { println!("{}", n); }
    n += 1;
}

// A return whose track crosses other columns' dashed legs.
struct Holder<'a> { r: &'a String }
fn crossing_dashed_legs(c: bool, d: bool) {
    let s = String::from("a");
    let t = String::from("b");
    let h = Holder { r: &s };
    if c {
        let u = t.len();
    } else if d {
        println!("{}", h.r);
    } else {
        drop(t);
    }
}

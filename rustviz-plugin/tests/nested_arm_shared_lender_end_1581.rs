// #1581: a reference last used in an arm of a nested conditional (an
// `else if` chain, an `if` inside an arm, a let-else inside a `match`
// arm) while another reference to the same lender lives on. Its end
// gives nothing back (the other reference still borrows the lender), and
// such an end was drawn only in the outer conditional's arms: in the
// nested arms r2 held its reference on to the join, which said "r2 isn't
// moved in any of the branches above", and the outer join had no dot.
fn else_if_chain(a: bool, b: bool) {
    let s = String::new();
    let r1 = &s;
    let r2 = &s;
    if a {
    } else if b {
    } else {
        let n = r2.len();
    }
    println!("{}", r1);
}

fn if_in_match_arm(k: u8, b: bool) {
    let t = String::new();
    let q1 = &t;
    let q2 = &t;
    match k {
        0 => {}
        _ => {
            if b {
                println!("{}", q2);
            }
        }
    }
    println!("{}", q1);
}

// Two reference parameters both borrow from the caller.
fn push_x(s: &mut String) { s.push('x'); }

fn params(p: &String, m: &mut String, a: bool, b: bool) {
    if a {
    } else if b {
        push_x(&mut *m);
    } else {
    }
}

// All on one line: the end stays on the join row, as on main (a one-line
// conditional's per-arm ends need analysis RustViz doesn't do, #1572).
fn params_one_line(p2: &String, m2: &mut String, a: bool, b: bool) { if a { } else if b { } else { let n = m2.len(); } }

fn let_else_in_arm(r: &String, m3: &mut String, o2: Option<String>, k: u8) {
    match k {
        0 => {
            let Some(x2) = o2 else {
                let t = std::mem::replace(m3, String::new());
                return;
            };
        }
        _ => {}
    }
}

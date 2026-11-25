// A reference whose old loan is still live into the arm that re-points
// it (`let r2 = r; r = &t;`): the loan ends in that arm, and on the other
// paths before their arms run, also in the arms of an `else if` chain or
// an `else { if .. }` (#1580). The other arms' legs of r used to hold the
// reference down to the join, which had no dot, and s stayed shared past
// the `if`.

fn else_if_chain(s: String, t: String, a: bool) {
    let mut r = &s;
    if r.is_empty() {
        let r2 = r;
        r = &t;
    } else if a {
    } else {
    }
}

fn plain_else(s: String, t: String) {
    let mut r = &s;
    if r.is_empty() {
        let r2 = r;
        r = &t;
    } else {
    }
}

// The same lender again in a nested arm (`r = &s;`): the old loan still
// ends in the arms that don't re-point r.
fn same_lender_nested(s: String, a: bool, b: bool) {
    let mut r = &s;
    if a {
        let n = r.len();
    } else if b {
        r = &s;
        println!("{}", r);
    } else {
        println!("{}", r);
    }
}

// r3's last use is the `else if` condition, on the line the arms share:
// it is on the other arm's path, so the new loan of s2 that r2 takes in
// the first arm gives s2 back when it ends.
fn shared_line_use(p: String, mut s2: String) {
    let mut r2 = &p;
    if s2.len() > 1 {
        let r3 = &s2;
        if s2.len() > 1 {
            r2 = &s2;
        } else if r3.is_empty() {
            let v = vec![s2, String::from("ab")];
        } else {
        }
    }
}

fn main() {}

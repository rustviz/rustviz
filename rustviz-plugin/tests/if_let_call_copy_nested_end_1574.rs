// #1574: `if let Some(x) = v.first()` (x copied out of the call's
// result, borrowing v), x last used in a nested `if`. v had no column in
// the conditionals, so x's end was drawn on x's column between the
// nested arms' legs, and the path that skips the nested `if` had none:
// its join said "x isn't moved in any of the branches above". Now x's
// loan ends in each nested arm, and v gets it back on each path (on the
// `else` path of the `if let`, as that arm starts).
fn nested_if(v: Vec<String>, c: bool, s2: String) {
    if let Some(x) = v.first() {
        if c {
            drop(s2);
            let o = x.clone();
        }
    }
}

fn nested_match(w: Vec<String>, n: i32, s3: String) {
    if let Some(y) = w.get(0) {
        match n {
            0 => {
                drop(s3);
                let o = y.clone();
            }
            _ => {}
        }
    }
}

fn match_on_call(u: Vec<String>, c: bool, s4: String) {
    match u.first() {
        Some(z) => {
            if c {
                drop(s4);
                let o = z.clone();
            }
        }
        None => {}
    }
}

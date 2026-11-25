// A value that exists only in one arm (a pattern binding, or a `let` in
// the arm) lent to a holder declared before the conditional (#1498). The
// lender is drawn in its arm only: it was drawn forking at the `match` /
// `if` before it existed, with its events twice and a join saying it
// "still has no value".
fn match_binding() {
    let o = Some(String::from("abc"));
    let mut vr = vec!["x"];
    match o {
        Some(x2) => vr.push(x2.as_str()),
        None => {}
    }
}

fn match_binding_block() {
    let o = Some(String::from("hi"));
    let mut vr = vec!["hey"];
    match o {
        Some(x2) => {
            vr.push(x2.as_str());
        }
        None => {
        }
    }
}

fn tuple_binding() {
    let s = String::from("abc");
    let tp = (s, 1);
    let mut vr = vec!["x"];
    match tp {
        (x2, 0) => {
            vr.push(x2.as_str());
        }
        (y, _) => {}
    }
}

fn let_in_if_arm() {
    let c = true;
    let mut vr = vec!["x"];
    if c {
        let s4 = String::from("hey");
        vr.push(s4.as_str());
    } else {
    }
}

fn let_in_slice_arm(v: Vec<String>) {
    let s2 = String::from("hi");
    let mut vr = vec![s2.as_str(), "x"];
    match &v[..] {
        [] => {}
        [x1, rest @ ..] => {
            let s4 = String::from("hey");
            vr.push(s4.as_str());
        }
    }
}

fn let_in_first_of_three_arms() {
    let c = 3;
    let mut vr = vec!["x"];
    match c {
        0 => {
            let s4 = String::from("hey");
            vr.push(s4.as_str());
        }
        1 => {}
        _ => {}
    }
}

fn if_let_else() {
    let o = Some(String::from("abc"));
    let mut vr = vec!["x"];
    if let Some(x2) = o {
        vr.push(x2.as_str());
    } else {
    }
}

fn main() {}

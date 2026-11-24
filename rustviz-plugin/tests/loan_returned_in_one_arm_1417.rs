// A loan given back in one arm of a conditional, by something that has
// no column in it (a closure called there, `c()`; `rest` of `[x, rest @
// ..]`, whose loan x's use keeps alive), or by a reference declared in
// an outer arm: the lender's "no longer borrowed" was drawn on its
// column's centre, between the arm legs, on neither (#1417). Also a
// borrow whose last use is the condition (`vr.is_empty()`) drawn a row
// down, inside the arm, when the arm had rows inserted for its arrows.
fn closure_called_in_else() {
    let b = Box::new(String::from("abc"));
    let c = || b.len();
    if b.len() > 2 {
        println!("{}", b);
    } else {
        c();
    }
}

fn closure_called_in_match_arm() {
    let b = String::from("abc");
    let c = || b.len();
    match b.len() {
        0 => println!("{}", b),
        _ => { c(); }
    }
}

fn closure_called_in_else_if() {
    let b = String::from("abc");
    let c = || b.len();
    let n = 3;
    if n > 2 {
        println!("{}", b);
    } else if n > 1 {
        c();
    } else {
    }
}

fn mut_closure_called_in_then() {
    let mut b = String::from("abc");
    let mut c = || b.push('x');
    let k = true;
    if k {
        c();
    } else {
        b.push('y');
    }
}

fn rest_of_a_slice_pattern() {
    let s2 = String::from("x");
    let v = vec![String::from("x"), String::from("hey")];
    if let [x, _rest @ ..] = v.as_slice() {
        if s2.len() > 1 {
        } else {
            let _r = x;
        }
    }
}

fn ref_declared_in_outer_arm() {
    let mut s2 = String::from("abc");
    if s2.len() > 1 {
        let m = &mut s2;
        if m.len() > 2 {
        } else {
            println!("{}", m);
        }
    } else {
    }
}

fn condition_is_last_use() {
    let s = String::from("abc");
    let vr = vec![s.as_str(), "rust"];
    if vr.is_empty() {
        let s2 = s;
        let _c = || s2.len();
    }
}

fn condition_is_last_use_nested() {
    let s = String::from("abc");
    let t = true;
    if t {
        let r = &s;
        if r.is_empty() {
            let s2 = s;
            let _c = || s2.len();
        }
    }
}

fn main() {
    closure_called_in_else();
    closure_called_in_match_arm();
    closure_called_in_else_if();
    mut_closure_called_in_then();
    rest_of_a_slice_pattern();
    ref_declared_in_outer_arm();
    condition_is_last_use();
    condition_is_last_use_nested();
}

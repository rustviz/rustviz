// A variable declared in an arm (`let t = s;`) and moved inside an `if
// let` nested there, whose scrutinee borrows a variable from outside
// (`v.first()`): the end of that borrow (`_x` is unused, so it ends on
// the `if let` line) moved into the inner arm declaring `_x`, where v
// has no column of its own, and v's column never got its "no longer
// borrowed". The plugin panicked drawing the return arrow ("no layout
// column for the timeline of event N", #1418).
fn in_then_arm() {
    let s = String::from("ab");
    let v = vec![String::from("x")];
    let c = true;
    if c {
        let t = s;
        if let Some(_x) = v.first() {
            drop(t);
        }
    }
}

fn in_else_arm() {
    let s = String::from("ab");
    let v = vec![String::from("x")];
    let c = true;
    if c {
    } else {
        let t = s;
        if let Some(_x) = v.first() {
            drop(t);
        }
    }
}

fn two_levels_down() {
    let s = String::from("ab");
    let v = vec![String::from("x")];
    let c = true;
    if c {
        if c {
            let t = s;
            if let Some(_x) = v.first() {
                drop(t);
            }
        }
    }
}

fn lender_declared_in_the_arm() {
    let s = String::from("ab");
    let c = true;
    if c {
        let v = vec![String::from("x")];
        let t = s;
        if let Some(_x) = v.first() {
            drop(t);
        }
    }
}

fn params(v: Vec<String>, s: String, c: bool) {
    if c {
        let t = s;
        if let Some(_x) = v.first() {
            drop(t);
        }
    }
}

fn main() {
    in_then_arm();
    in_else_arm();
    two_levels_down();
    lender_declared_in_the_arm();
    params(vec![], String::new(), true);
}

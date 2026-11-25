// A reference last read in an `if` condition and re-pointed after the
// `if`: its old loan ends in the condition, before either arm runs, and
// gives s back there (#1586). s's arms used to start out "shared", and its
// trunk ran straight through the join.

fn drop_in_arm(mut s: String) {
    let mut r = &s;
    if r.is_empty() {
        drop(s);
        s = String::new();
    }
    r = &s;
}

fn with_else(mut s: String) {
    let mut r = &s;
    if r.is_empty() {
        drop(s);
        s = String::new();
    } else {
        s.push('x');
    }
    r = &s;
}

fn match_scrutinee(mut s: String) {
    let mut r = &s;
    match r.len() {
        0 => {
            drop(s);
            s = String::new();
        }
        _ => {}
    }
    r = &s;
}

fn main() {}

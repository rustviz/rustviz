// #1535: a `for` loop over a moved collection, inside an arm, with an
// early exit in its body: the loop's iterator (a temporary) exists on both
// paths through the body's `if`, so its legs are owner legs.
fn in_if(c: bool, d: bool) {
    let v = vec![1];
    if c {
        for e in v {
            if d { break; }
        }
    }
}

fn in_match(n: i32, d: bool) -> i32 {
    let w = vec![2];
    match n {
        0 => {
            for e in w {
                if d { return e; }
            }
        }
        _ => {}
    }
    0
}

fn in_let_if(d: bool) -> i32 {
    let u = vec![3];
    let x = if d {
        for e in u {
            if e > 1 { break; }
        }
        1
    } else {
        2
    };
    x
}

fn main() {
    in_if(true, false);
    in_match(0, true);
    in_let_if(true);
}

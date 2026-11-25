// #1567: a `match` whose first arm reads a reborrow of a reference
// parameter (`let r = &*p;`) and whose other arm uses the parameter: on
// the first arm's path p's loan lasts as long as r's, so p's borrow ends
// with r's, on r's last use. It was drawn at the arm's first line (its
// pattern's), before r gave the borrow back. The `if` form was right.
fn shared(p: &String, n: i32) {
    let r = &*p;
    match n {
        1 => {
            let k = r.len();
        }
        _ => {
            let k = p.len();
        }
    }
}

fn mutable(m: &mut String, n: i32) {
    let r2 = &*m;
    match n {
        1 => {
            let k = r2.len();
        }
        _ => {
            m.push('a');
        }
    }
}

fn option_arms(q: &String, o: Option<i32>) {
    let r3 = &*q;
    match o {
        Some(x) => {
            let k = r3.len();
        }
        None => {
            let k = q.len();
        }
    }
}

// A `return` directly in a loop body, with no branch around it (#1020).
// The loop body is drawn inline and may not run, so the `return`'s drops
// stay at the scope ends after it, which say what happens on each path:
// a variable the body declared is always dropped at the `return` (the body
// never reaches its `}`); one declared before the loop, the loop's
// iterator and its head's temporary are dropped there if it runs.

// v empty: g is dropped at the end. Otherwise s, the iterator (with the
// rest of v) and g are dropped at the `return`.
fn first_len(v: Vec<String>, g: String) -> usize {
    for s in v {
        return s.len();
    }
    g.len()
}

// An iterator variable, and the body's own variables.
fn first_with(v: Vec<String>, g: String) -> usize {
    let it = v.into_iter();
    for s in it {
        let t = String::from("t");
        let u = s;
        return u.len() + t.len();
    }
    g.len()
}

// A `continue` in a branch drops s and t there; the `return` drops them
// otherwise.
fn first_nonempty(v: Vec<String>, g: String) -> usize {
    for s in v {
        let t = String::from("t");
        if s.is_empty() {
            continue;
        }
        return s.len() + t.len();
    }
    g.len()
}

// Two `return`s: a is dropped at the outer one, or at the inner one if
// that runs first.
fn nested(rows: Vec<Vec<String>>, g: String) -> usize {
    for row in rows {
        let a = String::from("a");
        for s in row {
            return s.len();
        }
        return a.len();
    }
    g.len()
}

// A `while let` body.
fn last_len(mut stack: Vec<String>, g: String) -> usize {
    while let Some(s) = stack.pop() {
        return s.len();
    }
    g.len()
}

fn main() {
    let v = vec![String::from("x")];
    println!("{}", first_len(v.clone(), String::from("g")));
    println!("{}", first_with(v.clone(), String::from("g")));
    println!("{}", first_nonempty(v.clone(), String::from("g")));
    println!("{}", nested(vec![v.clone()], String::from("g")));
    println!("{}", last_len(v, String::from("g")));
}

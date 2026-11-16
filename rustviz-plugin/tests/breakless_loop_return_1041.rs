// A `return` directly in a `loop` that nothing breaks out of (#1041): the
// body always runs and a `return` is the only way out, so what the function
// owns is dropped on the `return`'s row and nothing after the loop runs, as
// for a body drawn inline that always returns (#1004). A `loop` with a
// `break`, one in an `if` branch, one a `break 'outer` leaves and one in a
// `for` body keep "if the `return` runs first": the code after can run.

// g and t are both dropped at the `return`.
fn lp(g: String) -> usize {
    loop {
        let t = String::from("t");
        return t.len() + g.len();
    }
}

// Two `return`s: v and g are dropped at the outer one, unless the inner one
// runs first.
fn two(v: Vec<String>, g: String) -> usize {
    loop {
        for s in &v {
            return s.len();
        }
        return g.len();
    }
}

// Nested: a (the outer body's) and g are dropped at the inner `return`.
// So is the let-extended temporary r borrows.
fn nest(g: String) -> usize {
    let r = &String::from("r");
    let k = 3;
    loop {
        let a = String::from("a");
        loop {
            return a.len() + g.len() + r.len() + k;
        }
    }
}

// Control: a `break` can end the loop, so b may reach the end.
fn brk(b: String, c: bool) -> usize {
    loop {
        if c {
            break;
        }
        return b.len();
    }
    b.len()
}

// Control: the `else` path skips the loop and reaches the end.
fn inif(h: String, c: bool) -> usize {
    if c {
        loop {
            return h.len();
        }
    }
    h.len()
}

// Control: `break 'outer` leaves the inner loop, and the `for` loop can end.
fn esc(v: Vec<i32>, e: String) -> usize {
    'outer: for x in v {
        loop {
            if x > 0 {
                break 'outer;
            }
            return e.len();
        }
    }
    e.len()
}

// A `loop` in a `for` body: the `for` body's u never reaches its `}`, but
// the `for` loop may not run, so f may reach the end.
fn infor(v: Vec<i32>, f: String) -> usize {
    for x in v {
        let u = String::from("u");
        loop {
            return u.len() + x as usize;
        }
    }
    f.len()
}

fn main() {
    println!("{}", lp(String::from("g")));
    println!("{}", two(vec![String::from("x")], String::from("g")));
    println!("{}", nest(String::from("g")));
    println!("{}", brk(String::from("b"), true));
    println!("{}", inif(String::from("h"), false));
    println!("{}", esc(vec![1], String::from("e")));
    println!("{}", infor(vec![1], String::from("f")));
}

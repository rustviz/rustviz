// A let-else whose `else` block drops and moves nothing, with a borrow
// made before it that is used after it (#1573). On the `else` path the
// borrow is never used again, so it ends as the block starts and the
// lender can be used there; on the matched path it lives on. Drawn as a
// branch (as `if c { ..; return; }` is, #882) so the borrow can end on
// the `else` path only. It used to be drawn inline: the lender was read
// while still drawn borrowed (State::Invalid).

fn reborrow_return(o: Option<i32>, s: &mut String) {
    let m = &mut *s;
    let Some(x) = o else {
        println!("{}", s);
        return;
    };
    m.push('a');
    println!("{}", x);
}

fn shared_write_in_else(o: Option<i32>, s: &mut String) {
    let r = &*s;
    let Some(x) = o else {
        s.push('!');
        return;
    };
    println!("{} {}", r, x);
}

fn maybe(s: String) -> Option<String> {
    Some(s)
}

fn loop_break() {
    let mut o = Some(String::from("x"));
    for _i in 0..2 {
        let m = o.as_mut().unwrap();
        let Some(x) = maybe(String::from("abc")) else {
            println!("{:?}", o);
            break;
        };
        m.push_str(&x);
    }
}

fn loop_continue(v: Vec<Option<i32>>) {
    let mut total = String::new();
    for e in v {
        let t = &mut total;
        let Some(n) = e else {
            println!("{}", total);
            continue;
        };
        t.push_str(&n.to_string());
    }
}

// The borrow made before the `if` whose arm holds the let-else.
fn in_an_arm(c: bool, o: Option<i32>, s: &mut String) {
    let m = &mut *s;
    if c {
        let Some(x) = o else {
            println!("{}", s);
            return;
        };
        m.push_str(&x.to_string());
    }
}

fn panics_in_else(o: Option<i32>, s: &mut String) {
    let m = &mut *s;
    let Some(x) = o else {
        println!("{}", s);
        panic!("none");
    };
    m.push_str(&x.to_string());
}

// The same for an `if` whose body panics (#882 covered `return`, `break`
// and `continue` only).
fn if_panics(o: Option<i32>, s: &mut String) {
    let m = &mut *s;
    if o.is_none() {
        println!("{}", s);
        panic!("none");
    }
    m.push('a');
}

// s is a `&mut` whose referent r shares after the join: its bracket
// there still says it holds a mutable reference (#1591).
fn if_returns_shared(o: Option<i32>, s: &mut String) {
    let r = &*s;
    if o.is_none() {
        s.push('!');
        return;
    }
    println!("{}", r);
}

fn main() {
    let mut s = String::from("a");
    reborrow_return(Some(1), &mut s);
    shared_write_in_else(None, &mut s);
    loop_break();
    loop_continue(vec![Some(1), None]);
    in_an_arm(true, Some(2), &mut s);
    panics_in_else(Some(1), &mut s);
    if_panics(Some(1), &mut s);
    if_returns_shared(Some(1), &mut s);
}

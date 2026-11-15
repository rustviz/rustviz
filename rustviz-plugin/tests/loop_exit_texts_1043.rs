// A `loop` with no `break` whose only ways out are drawn in a branch (a
// `return` or panic in an `if`) or are a `?` (#1043): the code after the
// loop never runs, so the scope ends there aren't drawn. Each exit's own
// dots say what it drops; if no exit runs, the loop runs forever and
// nothing is dropped.

// g is dropped at the `return` on line 11, nowhere else.
fn onlybr(g: String, c: bool) -> usize {
    loop {
        if c {
            return g.len();
        }
    }
}

// A local too: p and h at the `return` on line 21.
fn local(p: String, c: bool) -> usize {
    let h = String::from("h");
    loop {
        if c {
            return p.len() + h.len();
        }
    }
}

// In a `for` body: the body's `}` (line 37) is never reached, so u is
// dropped only at the `return` on line 34. f is dropped at the end if v
// is empty.
fn in_for(v: Vec<u8>, f: String, c: bool) -> usize {
    for x in v {
        let u = String::from("u");
        loop {
            if c {
                return u.len() + x as usize;
            }
        }
    }
    f.len()
}

// A panic in a branch: q is dropped there (as the panic unwinds).
fn pan(q: String, c: bool) -> usize {
    loop {
        if c {
            panic!("stop {}", q.len());
        }
    }
}

// Only a `?`: w is dropped there if it returns early.
fn only_try(w: String, x: Option<u8>) -> Option<u8> {
    loop {
        x?;
    }
}

// A `?` before a `return` directly in a loop body (#1044): the scope ends
// after the `return` say the drop happens there, but if the `?` returns
// early it happens at the `?` instead, so they name it. A variable
// declared after the `?` (or by its `let`), a `?` after the loop and a `?`
// in a closure aren't named.

// s and t: at the `return` on line 68, or at the `?` on line 67.
fn first_ok(v: Vec<String>, x: Option<usize>) -> Option<usize> {
    for s in v {
        let t = String::from("t");
        let y = x?;
        return Some(y + s.len() + t.len());
    }
    None
}

// Two `?`s: s is in scope at both, t and a only at the second.
fn two_tries(v: Vec<String>, x: Option<usize>, z: Option<usize>) -> Option<usize> {
    for s in v {
        let a = x?;
        let t = String::from("t");
        let b = z?;
        return Some(a + b + s.len() + t.len());
    }
    None
}

// A `while` loop.
fn while_try(mut n: usize, x: Option<usize>) -> Option<usize> {
    while n > 0 {
        let t = String::from("t");
        let y = x?;
        n -= 1;
        return Some(y + t.len());
    }
    None
}

// A break-less `loop` in a `for` body: u at the `return` on line 102, or
// at the `?` on line 101.
fn nested(v: Vec<u8>, x: Option<usize>) -> Option<usize> {
    for e in v {
        let u = String::from("u");
        loop {
            let y = x?;
            return Some(y + u.len() + e as usize);
        }
    }
    None
}

// Controls: a `?` after the loop, and one in a closure, can't return
// before the `return`.
fn try_after(v: Vec<String>, x: Option<usize>) -> Option<usize> {
    for s in v {
        let t = String::from("t");
        return Some(s.len() + t.len());
    }
    let y = x?;
    Some(y)
}

fn closure_try(v: Vec<String>, x: Option<usize>) -> Option<usize> {
    for s in v {
        let t = String::from("t");
        let f = || -> Option<usize> { let q = x?; Some(q) };
        return Some(s.len() + t.len() + f().unwrap_or(0));
    }
    None
}

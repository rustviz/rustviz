// #1458: `let Some(f) = p else { return };` with p = `v.first_mut()`, an
// `Option<&mut String>` the pattern moves f out of. On the `return` path
// the pattern didn't match: p wasn't moved and isn't used again, so its
// loan of v has ended, and the `return` drops v (a Vec that owns its
// Strings). v was still drawn lent by p when it was dropped, and the drop
// said "v goes out of scope. No resource is dropped." p now gives v back
// first.
fn moved() {
    let mut v = vec![String::from("a")];
    let p = v.first_mut();
    let Some(f) = p else { return };
    f.push('x');
    println!("{:?}", v);
}

// A `Result` holder, and a panic instead of a `return` (the panic unwinds,
// dropping w).
fn result_panic() {
    let mut w = vec![String::from("a")];
    let q = w.last_mut().ok_or(0);
    let Ok(g) = q else { panic!() };
    g.push('x');
    println!("{:?}", w);
}

// The `else` block reads the holder first: its loan ends after that read.
fn read_in_else() {
    let mut u = vec![String::from("a")];
    let o = u.first_mut();
    let Some(h) = o else {
        println!("{:?}", o);
        return;
    };
    h.push('x');
    println!("{:?}", u);
}

// A holder of two loans (`zip`), inside an `if`.
fn two_lenders(c: bool) {
    let mut a = vec![String::from("a")];
    let mut b = vec![String::from("b")];
    if c {
        let pr = a.first_mut().zip(b.first_mut());
        let Some((x, y)) = pr else { return };
        x.push_str(y);
    }
    println!("{:?} {:?}", a, b);
}

// No holder: the call's result is matched directly. On the `else` path
// it was `None`, and its loan of s ended with it (no arrow: nothing with
// a column gives s back).
fn direct() {
    let mut s = vec![String::from("a")];
    let Some(k) = s.first_mut() else { return };
    k.push('x');
    println!("{:?}", s);
}

// The same with a shared borrow: the drop's text was right, but t was
// still drawn shared when the `return` dropped it.
fn direct_shared() {
    let t = vec![String::from("a")];
    let Some(m) = t.first() else { return };
    println!("{} {:?}", m, t);
}

fn main() {
    moved();
    result_panic();
    read_in_else();
    two_lenders(true);
    direct();
    direct_shared();
}

// `let (a, b) = if …` takes each branch's value apart in that branch
// (#1007): the if branch's tuple t moves t.0 into a and t.1 into b, so
// nothing is dropped where t goes out of scope; the else branch moves h
// into a and k into b, once each. Only one branch runs, so f, g, h and k
// are each dropped at the end of the function only if their branch didn't
// run (a runtime drop flag).
fn tup(c: bool, f: String, g: String, h: String, k: String) {
    let (a, b) = if c {
        let t = (f, g);
        t
    } else {
        (h, k)
    };
    drop(a);
    drop(b);
}

// The same with a `match`.
fn tup_match(n: i32, p: String, q: String, r: String, s: String) {
    let (x, y) = match n {
        0 => (p, q),
        _ => (r, s),
    };
    drop(x);
    drop(y);
}

// An arm's binding taken apart: pr.0 goes to m, pr.1 to w.
fn tup_opt(o: Option<(String, String)>) {
    let (m, w) = match o {
        Some(pr) => pr,
        None => return,
    };
    drop(m);
    drop(w);
}

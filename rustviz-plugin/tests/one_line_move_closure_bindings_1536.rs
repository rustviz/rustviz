// A `move` closure made on a one-line arm whose pattern binds two
// references (#1536): the closure is bound on the row its capture arrow
// ends on. It was bound on the row below the stacked arrows, with its
// scope end, so the arrow ended at no dot of f.
fn if_let_two_bindings(p: Option<(String, String)>) {
    let s = String::new();
    if let Some((x, y)) = &p { let f = move || s; }
}

fn slice_rest(mut v: Vec<String>) {
    let s = v.remove(0);
    if let [x, rest @ ..] = v.as_slice() { let c = move || s; }
}

fn match_arm(p: Option<(String, String)>) {
    let s = String::new();
    match &p { Some((x, y)) => { let f = move || s; } None => {} }
}

// A Copy capture.
fn copied(p: Option<(String, String)>) {
    let n = 5;
    if let Some((x, y)) = &p { let f = move || n; }
}

fn main() {}

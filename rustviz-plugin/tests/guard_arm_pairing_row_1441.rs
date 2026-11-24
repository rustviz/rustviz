// #1441: a one-line match arm with several arrows on its line (a guard
// reading the pattern's binding, a use of it) whose value the `let` is
// paired with after the arm is visited: a variable the arm declares
// (`{ let y = ..; y }`, #994) or a temporary it makes (#1397). The value's
// arrival from a call (`String::from -> y`) draws no arrow between two
// columns, so it went on the row below the line's stacked arrows, a row
// after `y -> r` moved it away: y was moved before it had a value. It now
// goes on the line's own row, like a binding the line's arrows move out of
// (#731).
fn deferred_var(o: Option<i32>) {
    let s = String::from("s");
    let r = match o {
        Some(n) if n > 0 => { let y = String::from("y"); y }
        _ => s,
    };
    println!("{}", r);
}

fn branch_temp(o: Option<i32>) {
    let s = String::from("s");
    let r = match o {
        Some(n) if n > 0 => &String::from("pos"),
        _ => &s,
    };
    println!("{}", r);
}

// No guard, but the arm uses its binding on the same line.
fn binding_used(o: Option<i32>) {
    let s = String::from("s");
    let r = match o {
        Some(n) => { let y = String::from("y"); println!("{}", n); y }
        _ => s,
    };
    println!("{}", r);
}

// A one-line `if` whose condition reads a variable.
fn if_cond(c: bool) {
    let s = String::from("s");
    let r = if c && s.len() > 0 { let y = String::from("y"); y } else { s };
    println!("{}", r);
}

fn main() {
    deferred_var(Some(1));
    branch_temp(Some(1));
    binding_used(Some(1));
    if_cond(true);
}

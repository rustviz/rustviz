// #1500: a `let`-`else` whose `else` block leaves from inside a nested
// `if` / `match`. The pattern's binding (x) exists only after the `let`,
// on the path where the pattern matched: in the `else` block it borrows
// nothing, so it has no borrow to end there.
fn first(o: Option<String>, c: bool) -> usize {
    let Some(x) = &o else {
        if c { return 1; }
        return 2;
    };
    x.len()
}

fn second(mut p: Option<String>, c: u8) -> usize {
    let Some(y) = &mut p else {
        match c {
            0 => return 1,
            _ => {}
        }
        return 2;
    };
    y.push('!');
    y.len()
}

fn third(q: Option<String>, c: bool) -> usize {
    let Some(z) = q.as_ref() else {
        if c { panic!("none"); }
        return 2;
    };
    z.len()
}

fn main() {
    first(None, true);
    second(None, 0);
    third(Some(String::new()), true);
}

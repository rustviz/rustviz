// A let-else drawn as a branch binds its pattern where the `else` block
// joins (the `};` row). A borrow the pattern binds starts there, but MIR
// ends an unused binding's loan on the `let` line, so the end was drawn
// two rows above the start (#1408).
fn unused_binding() {
    let o = Some(String::from("hi"));
    let Some(_x) = &o else {
        return;
    };
    println!("{:?}", o);
}

fn mut_gen<T>(_t: &mut T) {}

fn after_a_mutable_borrow() {
    let mut o: Option<String> = None;
    mut_gen(&mut o);
    let Some(_x2) = &o else {
        return;
    };
}

fn else_moves_the_scrutinee() {
    let o = Some(String::from("x"));
    let Some(_x) = &o else {
        let _s2 = match o {
            Some(y) => y,
            None => String::new(),
        };
        return;
    };
}

fn through_a_call() {
    let v = vec![String::from("rust")];
    let Some(_x) = v.first() else {
        return;
    };
    println!("{}", v.len());
}

fn main() {
    unused_binding();
    after_a_mutable_borrow();
    else_moves_the_scrutinee();
    through_a_call();
}

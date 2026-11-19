// #1200: `Some(pr) => pr,` binds pr, takes it apart into pr.0 and pr.1
// and moves those into m and w, on one line. pr.0's binding was drawn
// on the row below its move (read top to bottom, moved before it had a
// value); it now shares the move's dot, on the arm's row.
fn tup_opt(o: Option<(String, String)>) {
    let (m, w) = match o {
        Some(pr) => pr,
        None => return,
    };
    drop(m);
    drop(w);
}

// The same in a block arm: `let (m, w) = pr;` after pr is bound.
fn arm_moves_parts(o: Option<(String, String)>) {
    match o {
        Some(pr) => { let (m, w) = pr; drop(m); drop(w); }
        None => {}
    }
}

// #1223: an arm binding two names from one parent on one line. The
// println! borrows and the drops of a and b were drawn on a row inserted
// below for the second arrow (no code on it), which `e -> b` ran
// through. Now everything that happens to a shares one dot on the arm's
// row: in a lone arm, in a match with another arm, and in an `if let`.
enum F { V(String, String) }
enum E { V(String, String), W }

fn single_arm(e: F) {
    match e {
        F::V(a, b) => println!("{} {}", a, b),
    }
}

fn two_arms(e: E) {
    match e {
        E::V(a, b) => println!("{} {}", a, b),
        E::W => {}
    }
}

fn if_let(e: E) {
    if let E::V(a, b) = e { println!("{} {}", a, b); }
}

fn main() {
    tup_opt(Some((String::new(), String::new())));
    arm_moves_parts(None);
    single_arm(F::V(String::new(), String::new()));
    two_arms(E::W);
    if_let(E::W);
}

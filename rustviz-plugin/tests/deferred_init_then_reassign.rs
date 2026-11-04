// Deferred initialization followed by real reassignments (#234). The
// first `s = ...` initializes s (nothing to drop). The second replaces
// a value s owns, so the old String is dropped there. Same after an
// if/else that initializes t in both branches: `t = ...` afterwards
// drops the String t got in the branch.

fn main() {
    let mut s;
    s = String::from("first");
    println!("{}", s);
    s = String::from("second");
    println!("{}", s);

    let c = true;
    let mut t;
    if c {
        t = String::from("yes");
    } else {
        t = String::from("no");
    }
    println!("{}", t);
    t = String::from("later");
    println!("{}", t);
}

// An `if`/`else` whose `else` is on the next line (#354): the join is
// one line below the split, so there is no row between them for the
// branch columns. Used to panic ("min > max") in compute_branch_states.
fn main() {
    let n = 3;
    let s = if n > 0 { String::from("a") }
            else { String::from("b") };
    println!("{}", s);
}

// Which variables of the same name share a column, and what a
// numbered column's header says (#173, #308). A subscript means the
// variable shadows an earlier one that is still in scope.

// `s₂` (a re-let) shadows `s`. The two inner blocks each shadow `s₂`;
// they are never in scope together, so they share one column, `s₃`.
fn shadow_then_siblings() {
    let s = String::from("a");
    let s = s.len();
    {
        let s = 5;
        println!("{}", s);
    }
    {
        let s = 6;
        println!("{}", s);
    }
    println!("{}", s);
}

// The two arms of an `if` shadow nothing: one column `x`, declared on
// two lines.
fn if_else_arms(c: bool) {
    if c {
        let x = String::from("a");
        println!("{}", x);
    } else {
        let x = String::from("b");
        println!("{}", x);
    }
}

fn main() {
    shadow_then_siblings();
    if_else_arms(true);
}

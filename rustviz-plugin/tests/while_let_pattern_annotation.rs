// Regression for: pattern bindings inside `while let` / `if let`
// guards weren't annotated in the code panel because the
// surrounding desugared If carries `DesugaringKind::WhileLoop` /
// CondTemporary and `annotate_expr` bailed on `from_expansion`
// before reaching the user-written pat / scrutinee / body.
fn consume(_s: String) {}

fn main() {
    let mut stack = vec![String::from("a"), String::from("b")];
    while let Some(s) = stack.pop() {
        consume(s);
    }
}

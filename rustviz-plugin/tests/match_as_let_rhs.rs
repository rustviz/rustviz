// `match` as the RHS of a `let` (issue #87). Pre-fix: zero `Move`
// tooltips because match_rhs had no ExprKind::Match arm; arm labels
// rendered as raw AST tokens (`Int(Pu128(0), Unsuffixed)`, `Wild`).

fn main() {
    let n = 3;
    let s = match n {
        0 => String::from("zero"),
        _ => String::from("other"),
    };
    println!("{}", s);
}

/* --- BEGIN Variable Definitions ---
Owner x{_}, Owner y{_}
 --- END Variable Definitions --- */
fn main() {
    let x = 5; // !{ Duplicate(None->x) }
    let y = x; // !{ Duplicate(x->y) }
} /* !{
    GoOutOfScope(x),
    GoOutOfScope(y)
} */
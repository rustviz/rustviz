/* --- Variable Definitions ---
![
Owner x{_, Copy}
Owner y{_, Copy}
]
*/
fn main() {
    let x = 5; // !{ Duplicate(None->x) }
    let y = x; // !{ Duplicate(x->y) }
} /* !{
    GoOutOfScope(x),
    GoOutOfScope(y)
} */
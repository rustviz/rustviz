/* --- Variable Definitions ---
![
Owner x{_, Copy}
Owner y{_, Copy}
]
*/
fn main() {
    let x = 5; // ![2]{ Duplicate(None->x) }
    let y = x; // ![3]{ Duplicate(x->y) }
} /* ![4]{
    GoOutOfScope(x),
    GoOutOfScope(y)
} */
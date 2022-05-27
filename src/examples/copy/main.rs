/* --- BEGIN Variable Definitions ---
Owner x; Owner y
--- END Variable Definitions --- */
fn main() {
    let x = 5; // !{ Bind(x|true) }
    let y = x; // !{ Copy(x->y) }
} /* !{
    GoOutOfScope(x),
    GoOutOfScope(y)
} */
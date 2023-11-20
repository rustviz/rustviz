/* --- BEGIN Variable Definitions ---
Owner x
--- END Variable Definitions --- */
fn main() {
    let x = 5; // !{ Bind(x) }
} // !{ GoOutOfScope(x) }
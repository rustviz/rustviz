/* --- BEGIN Variable Definitions ---
Owner mut x
--- END Variable Definitions --- */
fn main() {
    let mut x = 5; // !{ Bind(None->x) }
    x = 6; //OK !{ Bind(None->x) }
} // !{ GoOutOfScope(x) }
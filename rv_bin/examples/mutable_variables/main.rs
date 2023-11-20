/* --- BEGIN Variable Definitions ---
Owner mut x
--- END Variable Definitions --- */
fn main() {
    let mut x = 5; // !{ Bind(x) }
    x = 6; //OK !{ Bind(x) }
} // !{ GoOutOfScope(x) }
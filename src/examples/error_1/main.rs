/* --- BEGIN Variable Definitions ---
Owner x;
--- END Variable Definitions --- */
fn main() {
    let x = 5; // !{ Bind(x) }
    x = 6; // !{ Bind(x|false*cannot assign twice to immutable varible x) }
}// !{ GoOutOfScope(x) }

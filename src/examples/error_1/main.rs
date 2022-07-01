/* --- BEGIN Variable Definitions ---
Owner x;
--- END Variable Definitions --- */
fn main() {
    let x = 5; // !{ Bind(x) }
    x = 6; // !{ Bind(x|false) } ERROR: cannot assign twice to immutable variable x
}// !{ GoOutOfScope(x) }

/* --- BEGIN Variable Definitions ---
Owner x;
Owner mut y;
Function String::from()
--- END Variable Definitions --- */
fn main() {
    let x = String::from("hello"); // !{ Move(String::from()->x) }
    let mut y = String::from("test"); // !{ Move(String::from()->y) }
    y = x; // !{ Move(x->y) }
} // !{ GoOutOfScope(x), GoOutOfScope(y) }
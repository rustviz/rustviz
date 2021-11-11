/* --- BEGIN Variable Definitions ---
Owner x;
Closure ||;
Owner y;
Function equal_to_x();
Function String::from();
--- END Variable Definitions --- */
fn main() {
    let x = String::from("Hello World!"); // !{ Move(String::from()->x) }
    let equal_to_x = move |z| {z == x}; // !{ MoveToClosure(x->||) }
    let y = String::from("Hello World!"); // !{ Move(String::from()->y) }
    equal_to_x(y); // !{ Move(y->equal_to_x()), GoOutOfScope(||) }
} // !{ GoOutOfScope(x), GoOutOfScope(y) }
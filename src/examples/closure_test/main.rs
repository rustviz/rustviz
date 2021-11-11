/* --- BEGIN Variable Definitions ---
Owner x;
Closure equal_to_x;
Owner y;
Function String::from();
--- END Variable Definitions --- */
fn main() {
    let x = String::from("Hello World!"); // !{ Move(String::from()->x) }
    let equal_to_x = move |z| z == x; // !{ Move(x->equal_to_x) }
    let y = String::from("Hello World!"); // !{ Move(String::from()->y) }
    equal_to_x(y); // !{ Move(y->equal_to_x) }
} // !{ GoOutOfScope(x), GoOutOfScope(y), GoOutOfScope(equal_to_x) }
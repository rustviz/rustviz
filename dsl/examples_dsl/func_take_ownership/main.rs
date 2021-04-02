/* --- BEGIN Variable Definitions ---
Owner s,
Owner some_string,
Function String::from(),
Function takes_ownership(),
Function println!()
--- END Variable Definitions --- */
fn main() {
    let s = String::from("hello"); // !{ Move(String::from()->s) }
    takes_ownership(s); // !{ Move(s->takes_ownership()) }
    // println!("{}", s) // won't compile if added
} // !{ GoOutOfScope(s) }

fn takes_ownership(some_string: String) { // !{ InitializeParam(some_string) }
    println!("{}", some_string)
} // !{ GoOutOfScope(some_string) }
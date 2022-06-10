/* --- BEGIN Variable Definitions ---
Owner x;
Owner some_string;
Function String::from();
Function takes_ownership();
Function println!()
--- END Variable Definitions --- */
fn main() {
    let x = String::from("ABC"); // !{ Move(String::from()->x) }
    let guard = 1;
    if guard == 1 { // !{ StartIf() }
        takes_ownership(x); // !{ Move(x->takes_ownership()) }
    } else { // !{ StartElse() }
        0
    } // !{ EndJoint() }
} // !{ GoOutOfScope(x)}

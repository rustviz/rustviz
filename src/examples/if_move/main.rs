/* --- BEGIN Variable Definitions ---
Owner x;
Owner guard;
Owner some_string;
Function String::from();
Function takes_ownership();
Function println!()
--- END Variable Definitions --- */
fn main() {
    let x = String::from("ABC"); // !{ Move(String::from()->x) }
    let guard = 1; // !{ Bind(guard) }
    if guard == 1 { // !{ StartIf() }
        takes_ownership(x); // !{ Move(x->takes_ownership()) }
    }
    else { // !{ StartElse() }
        0
    } // !{ EndJoint() }
} // !{ GoOutOfScope(x), GoOutOfScope(guard)}

fn takes_ownership(some_string: String) { // !{ Move(None->some_string) }
    println!("{}", some_string); // !{ PassByStaticReference(some_string->println!()) }
} // !{ GoOutOfScope(some_string) }

// 
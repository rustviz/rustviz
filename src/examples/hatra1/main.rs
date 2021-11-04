/* --- BEGIN Variable Definitions ---
Owner s; Owner mut x; Owner y; Owner some_string;
Function String::from();
Function takes_ownership();
Function println!()
--- END Variable Definitions --- */
 fn main() {
    let s = String::from("hello"); // !{ Move(String::from()->s) }
    takes_ownership(s); // !{ Move(s->takes_ownership()) }
    let mut x = 5; // !{ Bind(x) }
    let y = x; // !{ Copy(x->y) }
    x = 6; // !{ Bind(x) }
} // !{ GoOutOfScope(s), GoOutOfScope(x), GoOutOfScope(y) }

fn takes_ownership(some_string: String) { // !{ InitOwnerParam(some_string) }
    println!("{}", some_string); // !{ PassByStaticReference(some_string->println!()) }
} // !{ GoOutOfScope(some_string) }
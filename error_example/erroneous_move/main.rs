/* --- BEGIN Variable Definitions ---
Owner s;
StaticRef x;
Function String::from();
Function println!()
--- END Variable Definitions --- */
fn main() {
    let s = String :: from("hello"); // !{ Move(String::from()->s) }
    let x = &s; // !{ StaticBorrow(s->x) }

    let s2 = s; // !{ Move(s->s2) because x's lifetime hasn't ended (last use was on next line), it is an erroneous move }
    
    println!("{}", String::len(x)); // !{ PassByStaticReference(String::len(x)->println!()) }

} // !{ GoOutOfScope(s), GoOutOfScope(x) }
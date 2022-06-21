/* --- BEGIN Variable Definitions ---
Owner s;
Owner s2;
StaticRef x;
Function String::from();
Function println!();
Function String::len();
--- END Variable Definitions --- */
fn main() {
    let s = String :: from("hello"); // !{ Move(String::from()->s) }
    let x = &s; // !{ StaticBorrow(s->x) }
    let s2 = s; // !{ Move(s->s2|false) }
    println!("{}", String::len(x)); // !{ PassByStaticReference(String::len()->println!()) }

} // !{ GoOutOfScope(s), GoOutOfScope(x) }
/* --- BEGIN Variable Definitions ---
Owner s;
StaticRef x;
Owner s2;
Function String::from();
Function println!();
--- END Variable Definitions --- */
fn main() {
    let s = String::from("hello"); // !{ Move(String::from()->s) }
    let x = &s; // !{ StaticBorrow(s->x) }
    let s2 = s; // !{ Move(s->s2|false) } ERROR: s is borrowed
    println!("{}", x); // !{ PassByStaticReference(x->println!()) } 
} // !{ GoOutOfScope(s), GoOutOfScope(x), GoOutOfScope(s2) }

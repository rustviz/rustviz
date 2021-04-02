/* --- BEGIN Variable Definitions ---
Owner s;
Function String::from();
Function println!()
--- END Variable Definitions --- */
 fn main() {
    let s = String::from("hello");  // !{ Move(String::from()->s) }
    println!("{}", s); // !{ PassByStaticReference(s->println!()) }
} // !{ GoOutOfScope(s) }
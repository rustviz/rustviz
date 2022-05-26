/* --- BEGIN Variable Definitions ---
Owner x;
Function String::from();
Function println!()
--- END Variable Definitions --- */
fn main() {
    {
        let x = String::from("hello"); // !{ Move(String::from()->x) }
    } // !{ GoOutOfScope(x) }
    println!("{}",x); // !{ PassByStaticReference(x->println!()) }
}
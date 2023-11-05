/* --- BEGIN Variable Definitions ---
Owner x;
Owner y;
Function String::from();
Function println!()
--- END Variable Definitions --- */
fn main() {
    let x = String::from("hello"); // !{ Move(String::from()->x) }
    let y = x; // !{ Move(x->y) }
    println!("{}", y); // !{ PassByStaticReference(y->println!())  }
} // !{ GoOutOfScope(x), GoOutOfScope(y) }
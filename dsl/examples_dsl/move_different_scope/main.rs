/* --- BEGIN Variable Definitions ---
Owner x,
Owner y,
Owner z,
Function String::from(),
Function println!()
--- END Variable Definitions --- */
fn main() {
    let x = String::from("hello"); // !{ Move(String::from()->x) }
    let z = { // !{ Move(None->z) }
        let y = x; // !{ Move(x->y) }
        println("{}", y); // !{ PassByStaticReference(y->println!()) }
        // ...
    }; // !{ GoOutOfScope(y) }
    println!("Hello, world!");
} // !{ GoOutOfScope(x), GoOutOfScope(z) }
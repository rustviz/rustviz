/* --- BEGIN Variable Definitions ---
Owner x; Function println!()
--- END Variable Definitions --- */
fn main() {
    let x = 1;  // !{ Bind(x) }
    if x == 1 { // !{ StartIf() }
        println!("{}", x); // !{ PassByStaticReference(x->println!()) }
    }
    // !{ EndJoint() }
} // !{ GoOutOfScope(x) }
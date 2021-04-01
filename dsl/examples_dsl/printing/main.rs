/* --- BEGIN Variable Definitions ---
Owner x, Owner y, Function println!()
 --- END Variable Definitions --- */
fn main() {
    let x = 1; // !{ Bind(None->x) }
    let y = 2; // !{ Bind(None->y) }
    println!("x = {} and y = {}", x, y); // !{ PassByStaticReference(x->println!()), PassByStaticReference(y->println!()) }
} // !{ GoOutOfScope(x), GoOutOfScope(y) }
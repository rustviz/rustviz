/* --- BEGIN Variable Definitions ---
Owner mut x;
MutRef s;
Function String::from();
Function world();
Function println!();
Function push_str()
--- END Variable Definitions --- */
fn main() {
    let mut x = String::from("Hello"); // !{ Move(String::from()->x) }
    world(&mut x); // !{ PassByMutableReference(x->world()) }
    println!("{}", x); // !{ PassByStaticReference(x->println!()) }
} // !{ GoOutOfScope(x) }

fn world(s : &mut String) { // !{ InitRefParam(s) }
    s.push_str(", world"); // !{ PassByMutableReference(s->push_str()) }
} // !{ GoOutOfScope(s) }
/* --- BEGIN Variable Definitions ---
Owner mut x;
MutableRef y;
MutableRef z;
Function String::from();
Function println!();
Function String::push_str();
--- END Variable Definitions --- */
fn main() {
    let mut x = String::from("Hello"); // !{ Move(String::from()->x)}
    let y = &mut x; // !{ MutableBorrow(x->y) }
    let z = &mut x; // !{ MutableBorrow(x->z|false) } ERROR: y is still live
    String::push_str(y, ", world"); // !{ PassByMutableReference(y->String::push_str()), PassByStaticReference(String::from()->String::push_str()) }
    String::push_str(z, ", friend"); // !{ PassByMutableReference(z->String::push_str()|false), PassByStaticReference(String::from()->String::push_str()|false) }
    println!("{}", x); // !{ PassByStaticReference(x->println!()) }
  } // !{ GoOutOfScope(x), GoOutOfScope(y) }
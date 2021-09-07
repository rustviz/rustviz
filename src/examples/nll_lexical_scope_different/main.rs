/* --- BEGIN Variable Definitions ---
Owner mut x;
MutRef y;
MutRef z;
MutRef s;
Function String::from();
Function world();
Function push_str();
Function println!()
--- END Variable Definitions --- */
fn main() {
    let mut x = String::from("Hello"); // !{ Move(String::from()->x) }
    let y = &mut x; // !{ MutableBorrow(x->y) }
    world(y); // !{ PassByMutableReference(y->world()), MutableDie(y->x) }
    let z = &mut x; // OK, because y's lifetime has ended (last use was on previous line), !{ MutableBorrow(x->z) }
    world(z); // !{ PassByMutableReference(z->world()), MutableDie(z->x) }
    x.push_str("!!"); // Also OK, because y and z's lifetimes have ended, !{ PassByMutableReference(x->push_str()) }
    println!("{}", x); // !{ PassByStaticReference(x->println!()) }
} // !{ GoOutOfScope(x), GoOutOfScope(y), GoOutOfScope(z) }

fn world(s : &mut String) { // !{ InitRefParam(s) }
    s.push_str(", world"); // !{ PassByMutableReference(s->push_str()) }
} // !{ GoOutOfScope(s) }
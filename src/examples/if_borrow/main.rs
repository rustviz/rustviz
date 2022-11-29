/* --- BEGIN Variable Definitions ---
Owner mut x;
Owner mut y;
Owner mut z;
Owner guard;
Function String::from();
Function push_str();
--- END Variable Definitions --- */
fn main() {
    let mut x = String::from("ABC"); // !{ Move(String::from()->x) }
    let mut y = String::from("DEF"); // !{ Move(String::from()->y) }
    let mut z = &mut y; // !{ MutableBorrow(y->z) }
    let guard = 1; // !{ Bind(guard) }
    if guard == 1 { // !{ StartIf() }
        z = &mut x; // !{ MutableBorrow(x->z) }
        z.push_str(","); // !{ PassByMutableReference(z->push_str()) }
    }
    else { // !{ StartElse() }
        z.push_str(","); // !{ PassByMutableReference(z->push_str()) }
    } // !{ EndJoint() }
}// !{ GoOutOfScope(x), GoOutOfScope(y), GoOutOfScope(z), GoOutOfScope(guard)}
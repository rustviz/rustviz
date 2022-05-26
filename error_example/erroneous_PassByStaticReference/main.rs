/* --- BEGIN Variable Definitions ---
Owner x;
MutableRef x;
Function String::from();
Function String::push_str();
Function f();
Function println!()
--- END Variable Definitions --- */
fn main() {
    let mut x = String::from("hello"); // !{ Move(String::from()->x) }
    let y = &mut x; // !{ MutableBorrow(x->y) }

    f(&x); // !{ PassByStaticReference(s->s2) because y's lifetime hasn't ended (last use was on next line), it is an erroneous PassByStaticReference }
    
    String::push_str(y,", world"); // !{ PassByMutableReference(String::len(x)->println!()) }

} // !{ GoOutOfScope(x), GoOutOfScope(y) }

fn f(x : &String) { // !{ InitOwnerParam(x) }
    println!("{}",x); // !{ PassByStaticReference(x->println!()) }
} // !{ GoOutOfScope(x) }
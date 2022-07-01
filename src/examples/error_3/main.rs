/* --- BEGIN Variable Definitions ---
Owner mut x;
MutRef y;
StaticRef s;
Function String::from();
Function println!();
Function String::push_str();
Function f();
--- END Variable Definitions --- */
fn main() {
    let mut x = String::from("hello"); // !{ Move(String::from()->x)}
    let y = &mut x; // !{ MutableBorrow(x->y) }
    f(&x); // !{ PassByStaticReference(x->f()|false) } ERROR: y is still live
    String::push_str(y, ", world"); // !{ PassByMutableReference(y->String::push_str()), PassByStaticReference(String::from()->String::push_str()), MutableDie(y->x) }
} // !{ GoOutOfScope(x), GoOutOfScope(y) }
  
fn f(s : &String) { // !{ InitOwnerParam(s) }
    println!("{}", s); // !{ PassByStaticReference(s->println!()) }
} // !{ GoOutOfScope(s) }
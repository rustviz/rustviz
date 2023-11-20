/* --- BEGIN Variable Definitions ---
Owner x;
StaticRef s;
Function String::from();
Function f();
Function println!()
--- END Variable Definitions --- */
fn main() {
    let x = String::from("hello"); // !{ Move(String::from()->x) }
    f(&x);  // !{ PassByStaticReference(x->f()) }
    println!("{}", x); // !{ PassByStaticReference(x->println!()) }
} // !{ GoOutOfScope(x) }

fn f(s : &String) { // !{ InitRefParam(s) }
    println!("{}", *s); // !{ PassByStaticReference(s->println!()) }
} // !{ GoOutOfScope(s) }
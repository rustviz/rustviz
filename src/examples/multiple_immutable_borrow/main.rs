/* --- BEGIN Variable Definitions ---
Owner x;
StaticRef y;
StaticRef z;
StaticRef s1;
StaticRef s2;
Function String::from();
Function f();
Function println!()
--- END Variable Definitions --- */
fn main() {
    let x = String::from("hello"); // !{ Move(String::from()->x) }
    let y = &x; // !{ StaticBorrow(x->y) }
    let z = &x; // !{ StaticBorrow(x->z) }
    f(y, z); /* !{ PassByStaticReference(y->f()),
        PassByStaticReference(z->f()),
        StaticDie(y->x), StaticDie(z->x)
    } */
} // !{ GoOutOfScope(x), GoOutOfScope(y), GoOutOfScope(z) }

fn f(s1 : &String, s2 : &String) { // !{ InitRefParam(s1), InitRefParam(s2) }
    println!("{} and {}", s1, s2); // !{ PassByStaticReference(s1->println!()), PassByStaticReference(s2->println!()) }
} // !{ GoOutOfScope(s1), GoOutOfScope(s2) }
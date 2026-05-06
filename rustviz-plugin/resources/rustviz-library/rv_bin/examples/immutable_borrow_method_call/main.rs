/* --- BEGIN Variable Definitions ---
Owner s;
Owner len1;
Owner len2;
Function String::from();
Function String::len();
Function len();
Function println!()
--- END Variable Definitions --- */
fn main() {
    let s = String::from("hello"); // !{ Move(String::from()->s)  }
    let len1 = String::len(&s); // !{ PassByStaticReference(s->String::len()), Move(String::len()->len1) }
    let len2 = s.len(); // shorthand for the above // !{ PassByStaticReference(s->len()), Move(len()->len2) }
    println!("len1 = {} = len2 = {}", len1, len2); // !{ PassByStaticReference(len1->println!()), PassByStaticReference(len2->println!()) }
} // !{ GoOutOfScope(s), GoOutOfScope(len1), GoOutOfScope(len2) }
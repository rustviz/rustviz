/* --- BEGIN Variable Definitions ---
Owner mut s1;
Owner s2;
Function String::from();
Function String::push_str();
Function push_str();
Function println!()
--- END Variable Definitions --- */
fn main() { 
    let mut s1 = String::from("Hello"); // !{ Move(String::from()->s1) }
    let s2 = String::from(", world"); // !{ Move(String::from()->s2) }
    String::push_str(&mut s1, &s2);  // !{ PassByMutableReference(s1->String::push_str()), PassByStaticReference(s2->String::push_str()) }
    s1.push_str(&s2); // shorthand for the above, !{ PassByMutableReference(s1->push_str()), PassByStaticReference(s2->push_str()) }
    println!("{}", s1); // prints "Hello, world, world", !{ PassByStaticReference(s1->println!()) }
} // !{ GoOutOfScope(s1), GoOutOfScope(s2) }
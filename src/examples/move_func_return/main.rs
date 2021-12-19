/* --- BEGIN Variable Definitions ---
Owner x;
Owner s;
Function String::from();
Function f();
Function println!()
--- END Variable Definitions --- */
fn f() -> String {
    let x = String::from("hello"); // !{ Move(String::from()->x) }
    // ...
    x // !{ Move(x->None) }
}  // !{ GoOutOfScope(x) }
  
fn main() {
    let s = f(); // !{ Move(f()->s) }
    println!("{}", s); // !{ PassByStaticReference(s->println!()) }
} // !{ GoOutOfScope(s) }
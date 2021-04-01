/* --- BEGIN Variable Definitions ---
Owner mut s,
Owner some_string,
Function String::from(),
Function take_and_return_ownership(),
Function println!()
--- END Variable Definitions --- */
fn take_and_return_ownership(some_string : String) -> String { // !{ InitializeParam(some_string) }
    println!("{}", some_string);
    some_string
} // !{ Move(some_string->None) }
  
fn main() {
    let mut s = String::from("hello"); // !{ Move(String::from()->s) }
    s = take_and_return_ownership(s); // !{ Move(s->take_and_return_ownership()), Move(take_and_return_ownership()->s) }
    println!("{}", s);   // OK
} // !{ GoOutOfScope(s) }

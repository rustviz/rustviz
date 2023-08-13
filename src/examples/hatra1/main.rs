/* --- BEGIN Variable Definitions ---
Owner s; Owner mut x; Owner y; Owner some_string; Owner t; Owner z;
Function String::from();
Function takes_ownership();
Function println!()
--- END Variable Definitions --- */
 fn main() {
    let s = String::from("hello"); // !{ Move(String::from()->s) }
    takes_ownership(s); // !{ Move(s->takes_ownership()) }
    let mut x = 5; // !{ Bind(x) }
    let y = x; // !{ Copy(x->y) }
    x = 6; // !{ Bind(x) }
    (t,y) = mut_life(s,x,z); /* !{ Lifetime(<FUNC: mut_life>[s{9:13}*NAME* wow wow *DRPT* bow bow][x{10:12}][z{8:14}*NAME* z comes into scope *BODY* z is alive]->[t{8:11}][y{7:14}*NAME* y comes into scope *BODY* y is alive]) }*/
} // !{ GoOutOfScope(s), GoOutOfScope(x), GoOutOfScope(y) }

fn takes_ownership(some_string: String) { // !{ InitOwnerParam(some_string) }
    println!("{}", some_string); // !{ PassByStaticReference(some_string->println!()) }
} // !{ GoOutOfScope(some_string) }
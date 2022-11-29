/* --- BEGIN Variable Definitions ---
Owner string1;
Owner string2;
StaticRef x;
StaticRef y;
StaticRef result;
Function String::from();
Function longest();
Function println!()
--- END Variable Definitions --- */
fn main() {
    let string1 = String::from("abcd"); // !{ Move(String::from()->string1) }
    let string2 = String::from("xyz"); // !{ Move(String::from()->string2) }
                                       
    let result = longest(&string1, &string2); /* !{
                                                  PassByStaticReference(string1->longest()),
                                                  PassByStaticReference(string2->longest()),
                                                  Move(longest()->result)
                                              } */
    println!("The longest string is {}", result); // !{ PassByStaticReference(result->println!()) }
} /* !{ GoOutOfScope(result), GoOutOfScope(string1), GoOutOfScope(string2) } */

fn longest<'a>(x: &'a String, y: &'a String) -> &'a String { // !{ InitRefParam(x), InitRefParam(y) }
    if x.len() > y.len() { 
        x // !{ Move(x->result), GoOutOfScope(x) }
    } else {
        y // !{ Move(y->result), GoOutOfScope(y) }
    }
}

/* --- BEGIN Variable Definitions ---
Owner x;
Owner len_x;
Owner print_x;
Function String::from();
Function println!();
--- END Variable Definitions --- */
fn main() {
    
    let x = String::from("World"); // !{ Move(String::from()->x) }
    
    let len_x = || x.len(); // !{ StaticBorrow(x->len_x) }
    
    let print_x = || println!("{}", x); // !{ StaticBorrow(x->print_x) }

    println!("{}",len_x()); // !{StaticDie(len_x->x)}
    
    print_x();// !{StaticDie(print_x->x)}
    
}//!{ GoOutOfScope(x) , GoOutOfScope(len_x) , GoOutOfScope(print_x) } 
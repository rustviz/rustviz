/* --- BEGIN Variable Definitions ---
Owner mut x;
Owner f;
Owner world;
Function String::from();
Function push_str();
Function f();
Function println!()
--- END Variable Definitions --- */
fn main(){
    let mut x = String::from("Hello, "); // !{ Move(String::from()->x) }
    
    let mut f = |y:&String| x.push_str(y); //!{ MutableBorrow(x->f) }
    
    let world = String::from("World");  // !{ Move(String::from()->world) }
    
    append_to_x(); //!{ PassByStaticReference(world->f()) , MutableDie(f->x)}
    
    println!("{}",x); //!{PassByStaticReference(x->println!())}
}//!{ GoOutOfScope(x) , GoOutOfScope(f) , GoOutOfScope(world)}
/* --- BEGIN Variable Definitions ---
Owner mut s; StaticRef r1; StaticRef r2; MutRef r3;
Function String::from();
Function compare_strings();
Function clear_string()
--- END Variable Definitions --- */
fn main(){
    let mut s = String::from("hello"); // !{ Move(String::from()->s) }

    let r1 = &s; // !{ StaticBorrow(s->r1) }
    let r2 = &s; // !{ StaticBorrow(s->r2) }
    assert!(compare_strings(r1, r2)); /* !{
        PassByStaticReference(r1->compare_strings()),
        PassByStaticReference(r2->compare_strings()),
        StaticDie(r1->s), StaticDie(r2->s)
    } */

    let r3 = &mut s; // !{ MutableBorrow(s->r3) }
    clear_string(r3); // !{ PassByMutableReference(r3->clear_string()), MutableDie(r3->s) }
} // !{ GoOutOfScope(s), GoOutOfScope(r1), GoOutOfScope(r2), GoOutOfScope(r3) }
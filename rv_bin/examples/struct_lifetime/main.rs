/* --- BEGIN Variable Definitions ---
Struct i{p};
StaticRef first;
StaticRef n;
Function String::from();
Function println!()
--- END Variable Definitions --- */
struct Excerpt<'a> {
    p: &'a str,
}

fn some_function() {
    let n = String::from("Ok. I'm fine."); // !{ Move(String::from()->n) }
    let first = n.split('.').next().expect("Could not find a '.'"); // !{ StaticBorrow(n->first) }
    let i = Excerpt { // !{ Bind(i) }
        p: first, /* reference &str is copied to p !{ Copy(first->i.p) } */
    };
    println!("{}", first); // !{ PassByStaticReference(first->println!()) }
    // 'i' cannot be returned be returned
    // because the struct outlives 'n'
} /* !{
    StaticDie(i.p->n), StaticDie(first->n),
    GoOutOfScope(first), 
    GoOutOfScope(i), GoOutOfScope(i.p), GoOutOfScope(n)
} */

fn main() {
    some_function();
}
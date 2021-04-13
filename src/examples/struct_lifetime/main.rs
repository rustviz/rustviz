/* --- BEGIN Variable Definitions ---
Struct i{p};
StaticRef first;
StaticRef n;
Function String::from();
--- END Variable Definitions --- */
struct Excerpt<'a> {
    p: &'a str,
}

fn some_function() {
    let n = String::from("Ok. I'm fine."); // !{ Move(String::from()->n) }
    let first = n.split('.').next().expect("Could not find a '.'"); // !{ StaticBorrow(n->first) }
    let i = Excerpt { // !{ Bind(None->i) }
        p: first, /* reference &str is copied to p
                    !{ Copy(first->i.p), StaticReturn(first->n) } */
    };
} /* !{
    StaticReturn(i.p->n),
    GoOutOfScope(first), StructBox(i->i.p),
    GoOutOfScope(i), GoOutOfScope(i.p), GoOutOfScope(n)
} */

fn main() {
    some_function();
}
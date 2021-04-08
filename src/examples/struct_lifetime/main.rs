/* --- BEGIN Variable Definitions ---
Struct i{p};
StaticRef first;
Owner n;
Function String::from();
--- END Variable Definitions --- */
struct Excerpt<'a> {
    p: &'a str,
}

fn main() {
    let n = String::from("Ok. I'm fine."); // !{ Move(String::from()->n) }
    let first = n.split('.').next().expect("Could not find a '.'"); // !{ StaticBorrow(n->first) }
    let i = Excerpt { // !{ Bind(None->i) }
        p: first, // !{ StaticBorrow(first->p) }
    };
} /* !{
    StaticReturn(p->first), StaticReturn(first->n),
    GoOutOfScope(first), StructBox(i->p),
    GoOutOfScope(i),GoOutOfScope(p), GoOutOfScope(n)
} */
// A straight arrow between two columns that aren't neighbours runs
// across the columns in between. When one of those has a dot on the
// same line, the arrow hops over it, so the dot's tooltip can still
// be hovered (#252).
//
// `*r = y;`: the arrow returning r's borrow to x runs leftwards over
// y's column, where "y's resource is copied" sits on the same line.
// `let q = p;`: the move arrow from p to q runs rightwards over the
// p.name and p.email columns, which both get a dot on that line.
struct User {
    name: String,
    email: String,
}

fn main() {
    let mut x = 1;
    let y = 5;
    let r = &mut x;
    *r = y;
    println!("{}", x);
    let p = User { name: String::from("Ana"), email: String::from("ana@example.com") };
    let q = p;
    println!("{} {}", q.name, q.email);
}

// #1433: references inside a literal: a tuple in `vec![..]` holds its
// elements' borrows (f mutably, g immutably), and a `for` loop over an
// array or `vec![..]` literal of references borrows what they borrow,
// until the loop ends. Neither was drawn: f and g were never lent, and
// r's borrow ended "from nothing shown".
fn main() {
    let mut f = String::from("f");
    let g = String::from("g");
    let pairs = vec![(&mut f, &g)];
    println!("{}", pairs.len());
    let shared = vec![(&g, &g)];
    println!("{}", shared.len());
    let e = String::from("e");
    for r in [&e, &g] {
        println!("{}", r);
    }
    for r in vec![&g] {
        println!("{}", r);
    }
    for m in [&mut f] {
        m.push('!');
    }
    for (x, y) in [(&e, &g)] {
        println!("{} {}", x, y);
    }
}

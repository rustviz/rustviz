// #898: `x => ..` on `match *b`, with b a Box whose payload's parts have
// columns (b1.0, b1.1), moves the whole payload out, so b1.0 is moved on
// every path and only the Box's heap allocation is freed at b1's scope
// end. With a `_ =>` arm that moves nothing (b2), b2.0 is dropped there
// only on that path (a drop flag), and the allocation is freed either
// way. Also `let x = *b3;` in one branch and `take(*b4)` in another.
fn take(_p: (String, i32)) {}

fn main() {
    let b1 = Box::new((String::from("a"), 1));
    match *b1 {
        (s, 0) => println!("{}", s),
        x => println!("{}", x.0),
    }

    let b2 = Box::new((String::from("b"), 2));
    match *b2 {
        (s, 0) => println!("{}", s),
        x if x.1 == 1 => take(x),
        _ => {}
    }

    let k = 3;
    let b3 = Box::new((String::from("c"), 3));
    if k == 0 {
        let (s, _) = *b3;
        println!("{}", s);
    } else {
        let x = *b3;
        println!("{}", x.0);
    }

    let b4 = Box::new((String::from("d"), 4));
    if k == 1 {
        let (s, _) = *b4;
        println!("{}", s);
    } else {
        take(*b4);
    }
}

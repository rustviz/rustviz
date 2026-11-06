// #400: a temporary that a `let` borrows lives until the end of the
// enclosing block (temporary lifetime extension), so it gets a column
// of its own, labelled `(temporary)`, that is dropped at scope end.
// `show(&String::from(..))`'s temporary is dropped at the end of its
// statement and `&Point { x: 4 }` owns nothing to drop: no column.
struct Point {
    x: i32,
}

fn show(s: &str) {
    println!("{}", s);
}

fn main() {
    let r = &String::from("a");
    println!("{}", r);
    let m = &mut Vec::new();
    m.push(1);
    let s: &str = &String::from("x");
    show(&String::from("gone"));
    let q = &Point { x: 4 };
    {
        let inner = &String::from("in");
        println!("{}", inner);
    }
    println!("{:?} {} {}", m, s, q.x);
}

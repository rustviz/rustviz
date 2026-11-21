// `match &mut t { (a, b) => … }` makes one loan of all of t; a and b
// each reach a part of it, and t stays borrowed until the last use of
// either (#1215), as for `let (a, b) = &mut t;` (#1130).
struct Q { x: String, y: String }

fn tuple_mut() {
    let mut t = (String::from("a"), String::from("b"));
    match &mut t {
        (a, b) => {
            a.push('!');
            b.push('?');
        }
    }
    println!("{}", t.0);
}

fn tuple_shared() {
    let u = (String::from("c"), String::from("d"));
    match &u {
        (c, d) => {
            println!("{}", c);
            println!("{}", d);
        }
    }
}

fn struct_mut() {
    let mut s = Q { x: String::from("x"), y: String::from("y") };
    match &mut s {
        Q { x, y } => {
            x.push('1');
            y.push('2');
        }
    }
    println!("{}", s.x);
}

fn main() {
    tuple_mut();
    tuple_shared();
    struct_mut();
}

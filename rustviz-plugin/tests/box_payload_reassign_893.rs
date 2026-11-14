// #893: `*b = ...` after a part was moved out of a Box puts a whole new
// payload in it: the new parts go into the part columns and are dropped
// at the Box's scope end, as for `t = (..)` on a local tuple. Of the old
// payload, only what the move left is dropped at the assignment; the Box
// keeps its heap allocation.
struct P {
    a: String,
    b: String,
}

fn make() -> (String, i32) {
    (String::from("m"), 6)
}

fn main() {
    let mut bm = Box::new((String::from("a"), 1));
    let s2 = bm.0;
    *bm = (String::from("c"), 3);
    println!("{} {}", s2, bm.0);

    let mut b2 = Box::new((String::from("d"), 2));
    let (s3, _) = *b2;
    let o = (String::from("o"), 5);
    *b2 = o;
    println!("{} {}", s3, b2.0);

    let mut b3 = Box::new((String::from("e"), 4));
    let s4 = b3.0;
    *b3 = make();
    println!("{} {}", s4, b3.0);

    let mut bp = Box::new(P { a: String::from("x"), b: String::from("y") });
    let a = bp.a;
    *bp = P { a: String::from("z"), b: String::from("w") };
    println!("{} {} {}", a, bp.a, bp.b);
}

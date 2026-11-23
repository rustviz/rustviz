// #1354: fields the arms of a conditional point at different lenders.
// Each arm's borrows are drawn as they are (one lender per field there);
// at the fields' last use one return arrow per field, which names both
// lenders: after the join the field may point into either.
struct Two<'p> { a: &'p str, b: &'p str }

fn if_else() {
    let s1 = String::from("hi");
    let s2 = String::from("yo");
    let c = s1.len() > 1;
    let d = if c {
        Two { a: &s1, b: &s2 }
    } else {
        Two { a: &s2, b: &s1 }
    };
    println!("{} {}", d.a, d.b);
}

fn match_tuple_mut() {
    let mut s1 = String::from("hi");
    let mut s2 = String::from("yo");
    let n = s1.len();
    let t = match n {
        0 => (&mut s1, &mut s2),
        _ => (&mut s2, &mut s1),
    };
    t.0.push('!');
    t.1.push('?');
}

fn main() {
    if_else();
    match_tuple_mut();
}

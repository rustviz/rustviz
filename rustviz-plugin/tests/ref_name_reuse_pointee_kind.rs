// #917: variables of one name that are never in scope together share a
// column, and the column's texts come from the first of them. A `&i32`
// and a later `&Vec<i32>` (or a `&String` and a later `&i32`) shared
// one, so the later reference said "value" for a Vec, or "resource" for
// an i32 (#484: "resource" only for what needs drop).
fn main() {
    let v = vec![1, 2, 3];
    for x in v.iter() {
        println!("{}", x);
    }
    let u = vec![4, 5];
    let x = &u;
    let k = x.len();
    println!("{}", k);
    let words = vec![String::from("a"), String::from("b")];
    for w in words.iter() {
        println!("{}", w);
    }
    let n = 7;
    let w = &n;
    let t = w.to_string();
    println!("{}", t);
}

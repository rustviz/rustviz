// #899: in `let (Some(s), n) = t else { .. };` the nested refutable
// sub-pattern `Some(s)` takes the String inside t.0 only, as `if let
// (Some(s), n) = t` does: s is bound from t.0, not from all of t, and
// t.0 has nothing left to drop at its scope end. Same on a Box's
// payload (`*b`), on a struct field (`P { a: Some(a), .. }`), and for a
// Copy value copied out of a part (`(Some(k), s)`).
struct P {
    a: Option<String>,
    b: String,
}

fn main() {
    let t = (Some(String::from("a")), 1);
    let (Some(s), n) = t else { return; };
    println!("{} {}", s, n);

    let b = Box::new((Some(String::from("b")), 2));
    let (Some(s2), n2) = *b else { return; };
    println!("{} {}", s2, n2);

    let p = P { a: Some(String::from("c")), b: String::from("d") };
    let P { a: Some(a), .. } = p else { return; };
    println!("{}", a);

    let u = (Some(5), String::from("e"));
    let (Some(k), s3) = u else { return; };
    println!("{} {}", k, s3);
}

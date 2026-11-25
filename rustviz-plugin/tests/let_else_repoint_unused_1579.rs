// A let-else binding re-pointed before it is read (#1579): its first
// loan is never used, so rustc ends it where the binding is made (the
// join row of the drawn let-else, #1408), and the lender is free again
// there. The end used to be drawn on the `let` row, above the binding,
// and the lender was never given back.

fn from_a_call(v: Vec<String>, s2: String) {
    let Some(mut x) = v.first() else {
        return;
    };
    x = &s2;
    println!("{}", x);
}

fn through_as_ref(o: Option<String>, s2: String) {
    let Some(mut x) = o.as_ref() else {
        return;
    };
    x = &s2;
    println!("{}", x);
}

fn in_a_tuple_pattern(v: Vec<String>, s2: String) {
    let (Some(mut x), n) = (v.first(), 1) else {
        return;
    };
    x = &s2;
    println!("{} {}", x, n);
}

fn repointed_in_an_arm(v: Vec<String>, s2: String, c: bool) {
    let Some(mut x) = v.first() else {
        return;
    };
    if c {
        x = &s2;
        println!("{}", x);
    }
}

fn main() {
    from_a_call(vec![String::from("a")], String::from("b"));
    through_as_ref(None, String::from("b"));
    in_a_tuple_pattern(vec![], String::from("b"));
    repointed_in_an_arm(vec![], String::from("b"), true);
}

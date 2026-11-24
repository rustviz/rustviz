// #1393: `r = if c { .. } else { .. };` with r a reference that holds one
// already: each branch's value goes into r in that branch, as for `let r
// = if ..`, so a reference the branch declares is copied into r before it
// goes out of scope. It was drawn after the conditional, after y and x had
// gone out of scope.
fn first(v: &Vec<String>) -> &String {
    &v[0]
}

fn main() {
    let a = String::from("a");
    let b = String::from("b");
    let v = vec![String::from("v")];
    let mut r = &b;
    println!("{}", r);
    r = if a.len() > 1 { let y = &a; y } else if b.len() > 1 { first(&v) } else { &b };
    println!("{}", r);
    let o: Option<&String> = Some(&a);
    r = match o { Some(x) => x, None => &b };
    println!("{}", r);
}

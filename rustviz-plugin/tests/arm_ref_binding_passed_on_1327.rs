// #1327 siblings: a `match` arm hands on its binding, which borrows the
// scrutinee, as the match's value: as a call's argument, or returned from
// the function. The borrow goes with it: to `show` until it returns, or to
// the caller (it isn't given back to o at the end of the arm).
fn show(v: &String) {
    println!("{}", v);
}

fn pick<'a>(o2: &'a Option<String>, t2: &'a String) -> &'a String {
    match o2 { Some(x2) => x2, None => t2 }
}

fn main() {
    let o1: Option<String> = Some(String::from("hi"));
    let t1 = String::from("t");
    show(match &o1 { Some(x1) => x1, None => &t1 });
    let p = pick(&o1, &t1);
    println!("{}", p);
}

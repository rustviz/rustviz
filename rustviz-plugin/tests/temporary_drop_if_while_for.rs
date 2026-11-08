// #469: the temporary an `if let` / `while let` scrutinee builds, and
// the iterator a `for` loop makes from its head, are dropped without a
// column of their own; the drop is stated on a `(temporary N)` column.
fn make() -> String {
    String::from("m")
}

fn make_vec() -> Vec<String> {
    vec![String::from("a"), String::from("b")]
}

fn main() {
    if let Some(_) = Some(make()) {
        println!("matched");
    }
    if let Some(_) = Some(make()) {
        println!("a");
    } else {
        println!("b");
    }
    let mut n = 0;
    while let Some(_) = Some(make()) {
        n += 1;
        if n > 1 {
            break;
        }
    }
    for _ in make_vec() {
        println!("x");
    }
    let v = make_vec();
    for s in v {
        println!("{}", s);
    }
    for c in make().chars() {
        println!("{}", c);
    }
}

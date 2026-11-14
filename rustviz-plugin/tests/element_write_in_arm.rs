// An element written in place or assigned inside an `if` / `match`
// arm is drawn on the collection's column, as it is outside a branch
// (#860): `v[0] += 1` is an in-place update ("modified through"),
// `v[1] = x` an assignment to the place `v[1]`. For a Vec<String>
// the overwritten element's drop is stated too.
fn main() {
    let mut v = vec![1, 2, 3];
    let c = v.len() > 2;
    if c {
        v[0] += 1;
    } else {
        v[1] = 7;
    }
    let mut a = [1, 2, 3];
    match a[0] {
        1 => a[1] += 1,
        _ => a[2] = 9,
    }
    let mut w = vec![String::from("a"), String::from("b")];
    if c {
        w[0] = String::from("z");
    } else {
        w[1].push('x');
    }
    // Both arms assign a String element: this used to be refused
    // ("no layout column for the timeline of event N").
    let mut u = vec![String::from("a"), String::from("b")];
    if c {
        u[0] = String::from("z");
    } else {
        u[1] = String::from("y");
    }
    println!("{:?} {:?} {:?} {:?}", v, a, w, u);
}

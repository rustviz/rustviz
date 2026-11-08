// `break` and `continue` in an `if` without `else` (#429): `u` is
// dropped at the `break` and at the `continue`, on those paths only.

fn main() {
    let s = String::from("s");
    for i in 0..10 {
        let u = String::from("u");
        if i == 3 {
            break;
        }
        if i == 5 {
            continue;
        }
        println!("{}", u.len());
    }
    println!("{}", s);
}

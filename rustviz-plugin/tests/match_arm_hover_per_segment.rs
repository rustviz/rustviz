// #388: the same for a `match` arm. `v`'s column in the `_` arm is
// "owner" down to `let first = &v[0];`, then shared; each stretch
// shows its own tooltip when hovered.
fn main() {
    let v = vec![1, 2];
    let n = 2;
    match n {
        0 => {
            println!("zero");
        }
        _ => {
            println!("other");
            let first = &v[0];
            println!("{}", first);
        }
    }
    println!("{:?}", v);
}

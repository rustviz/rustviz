// The result of calling an `Fn` closure comes from the call (#299):
// `y` gets the closure's return value, and `add_one` itself isn't
// copied.

fn main() {
    let add_one = |x: i32| x + 1;
    let y = add_one(5);
    println!("{}", y);
}

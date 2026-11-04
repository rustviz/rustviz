// A Copy argument to a closure call (#258) is copied into the
// closure's parameter; it doesn't initialize `add_one`.

fn main() {
    let add_one = |x: i32| x + 1;
    add_one(5);
}

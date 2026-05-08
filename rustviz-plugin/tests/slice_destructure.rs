// Slice-pattern destructure of an array literal — companion to
// tuple_destructure (issue #86). Same shape: each pattern element
// pairs with the corresponding array element as if it were its
// own `let`.

fn main() {
    let [a, b] = [String::from("x"), String::from("y")];
    println!("{} {}", a, b);
}

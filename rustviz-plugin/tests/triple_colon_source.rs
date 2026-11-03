// The plugin prints `<code panel>:::<timeline panel>`; a `:::` in the
// user's source (this comment ::: and the string below) must not be taken
// as that separator (#181).
fn main() {
    let s = String::from("a:::b");
    let t = s;
    println!("{}", t);
}

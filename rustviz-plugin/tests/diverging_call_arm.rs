// A call to a `-> !` function has type `!`, so an arm that calls it
// diverges just like `panic!()` does. A function that merely panics at
// runtime but returns `()` is a normal call and still reaches the merge.
fn fail() -> ! {
    panic!("no")
}

fn main() {
    let n = 2;
    let s = match n % 2 {
        0 => String::from("even"),
        _ => fail(),
    };
    println!("{}", s);
}

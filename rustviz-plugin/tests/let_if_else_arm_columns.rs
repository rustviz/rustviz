// #369: `let s = if … { … } else { … };` with a multi-line `if` arm.
// The `else` column starts at `} else {`, not beside the `if` arm.
fn main() {
    let c = true;
    let s = if c {
        println!("a");
        String::from("yes")
    } else {
        String::from("no")
    };
    println!("{}", s);
}

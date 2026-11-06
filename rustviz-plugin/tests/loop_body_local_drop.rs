// `let`s inside a loop body go out of scope at the body's closing
// brace, so an owned value created there is dropped each iteration
// (#256). Before the fix `y`, `z` and `w` never went out of scope.
// `moved` is moved into `consume`, so it has nothing left to drop.

fn consume(_o: String) {}

fn main() {
    for i in 0..2 {
        let y = String::from("a");
        let moved = String::from("m");
        consume(moved);
    }
    let mut n = 0;
    while n < 1 {
        let z = String::from("b");
        n += 1;
    }
    loop {
        let w = String::from("c");
        break;
    }
}

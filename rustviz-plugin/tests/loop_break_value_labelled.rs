// A labelled `break` out of an inner loop hands the value to the outer
// loop's `let` (#431).

fn main() {
    let w = 'outer: loop {
        loop {
            break 'outer String::from("w");
        }
    };
    println!("{}", w);
}

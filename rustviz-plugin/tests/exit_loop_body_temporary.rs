// The `let`-extended temporary `String::from("c")` lives to the end of
// the loop body, but every iteration leaves earlier, through the
// `break` or the `continue`, which drop it (#631).

fn h(mut i: u32) -> usize {
    let mut n = 0;
    while i < 10 {
        let r = &String::from("c");
        n += r.len();
        i += 1;
        if i == 5 {
            break;
        } else {
            continue;
        }
    }
    n
}

fn main() {
    println!("{}", h(0));
}

// A `break` with a value in a branch of the loop (#431, #350): the
// value leaves the loop with the `break`, so `r` has it where the loop
// ends. Inside the loop `a` and `b` are still owned where the branches
// join; they were moved out only on the paths that left the loop.

fn main() {
    let a = String::from("a");
    let mut n = 0;
    let r = loop {
        n += 1;
        match n {
            3 => break a,
            _ => println!("again"),
        }
    };
    let b = String::from("b");
    let q = loop {
        n += 1;
        if n == 6 {
            break b;
        } else {
            println!("again");
        }
    };
    println!("{} {}", r, q);
}

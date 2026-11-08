// A `break` in a branch (#431) whose loop is the right side of an
// assignment, a tuple pattern or a call argument (#452): the value
// reaches the receiver where the loop ends, as for `let r = loop`.

fn take(s: String) {}

fn main() {
    let s = String::from("s");
    let mut r = String::from("old");
    let mut n = 0;
    r = loop {
        n += 1;
        if n == 3 {
            break s;
        }
    };
    let x = String::from("x");
    let (a, m) = loop {
        n += 1;
        if n == 6 {
            break (x, n);
        }
    };
    let z = String::from("z");
    take(loop {
        n += 1;
        if n == 9 {
            break z;
        }
    });
    println!("{} {} {}", r, a, m);
}

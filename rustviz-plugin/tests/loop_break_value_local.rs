// A `break` in a branch carries out a value declared inside the loop
// (#431). `break k` moves `k` into `r` on the iteration that ends the
// loop; every other iteration reaches the end of the body and drops its
// `k` there. In the second loop every iteration leaves through a
// `break` or a `continue` (which drops `j`), so nothing reaches the end
// of the body.

fn main() {
    let mut n = 0;
    let r = loop {
        n += 1;
        let k = String::from("k");
        if n == 3 {
            break k;
        }
    };
    let q = loop {
        n += 1;
        let j = String::from("j");
        if n == 6 {
            break j;
        } else {
            continue;
        }
    };
    println!("{} {}", r, q);
}

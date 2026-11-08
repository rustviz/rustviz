// Every iteration of each loop leaves through a `break` or a
// `continue`, which drop the body's `w` / `v` (#410). The body's
// closing brace is never reached, so nothing is dropped there (#631).

fn f(xs: &[u32]) -> usize {
    let mut n = 0;
    for x in xs {
        let w = String::from("a");
        n += w.len();
        if *x == 0 {
            break;
        } else {
            continue;
        }
    }
    n
}

fn g(mut i: u32) -> usize {
    let mut n = 0;
    loop {
        let v = String::from("b");
        n += v.len();
        i += 1;
        match i {
            5 => break,
            _ => continue,
        }
    }
    n
}

fn main() {
    println!("{} {}", f(&[1, 0]), g(0));
}

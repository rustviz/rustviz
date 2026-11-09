// #772: `for n in a` moves the Vec into the loop's iterator; each `n` is
// an i32 copied out, so no "Move from a to n" arrow. A String element
// is still moved out of the iterator into `s`.
fn sum(a: Vec<i32>) {
    for n in a {
        println!("{}", n);
    }
}

fn main() {
    let a = vec![1, 2];
    for n in a {
        println!("{}", n);
    }
    let pts = vec![(1, 2)];
    for p in pts {
        println!("{}", p.0);
    }
    let names = vec![String::from("x")];
    for s in names {
        println!("{}", s);
    }
    sum(vec![3]);
}

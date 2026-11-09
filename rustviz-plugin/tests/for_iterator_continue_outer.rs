fn main() {
    let rows = vec![vec![String::from("a")], vec![String::from("b")]];
    'outer: for row in rows {
        for s in row {
            if s == "a" {
                continue 'outer;
            } else {
                println!("{}", s);
            }
        }
    }
    println!("done");
}

fn named() {
    let mut n = 0;
    'scan: while n < 2 {
        n += 1;
        let it = vec![String::from("x"), String::from("y")].into_iter();
        for s in it {
            if s == "x" {
                continue 'scan;
            } else {
                println!("{}", s);
            }
        }
    }
}

// An unlabeled `continue` stays in the inner loop: no drop.
fn inner_continue() {
    let rows = vec![vec![String::from("a")]];
    for row in rows {
        for s in row {
            if s == "a" {
                continue;
            } else {
                println!("{}", s);
            }
        }
    }
}

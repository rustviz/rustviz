fn main() {
    let rows = vec![vec![String::from("a")], vec![String::from("b")]];
    'outer: for row in rows {
        for s in row {
            if s == "a" {
                break 'outer;
            } else {
                println!("{}", s);
            }
        }
    }
    println!("done");
}

fn first_x() {
    let it = vec![String::from("x"), String::from("y")].into_iter();
    'scan: loop {
        for s in it {
            if s == "x" {
                break 'scan;
            } else {
                println!("{}", s);
            }
        }
        break;
    }
    println!("done");
}

// The target loop's own iterator is a variable (#596).
fn outer_iter_var() {
    let outer_it = vec![vec![1], vec![2]].into_iter();
    'outer: for row in outer_it {
        for n in row {
            if n == 1 {
                break 'outer;
            } else {
                println!("{}", n);
            }
        }
    }
    println!("done");
}

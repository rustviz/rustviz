// A copied reference (`let mut r = x;`) re-pointed in one arm and read in
// another: on the reading path its borrow ends at that read, and x's loan,
// which r carries, ends with it; on the re-pointing path both end as the
// arm starts. The join says r's borrow ended in each branch (#1589). The
// reading arm used to end nothing, and r held a reference past the join.

fn read_in_else(s: String, t: String, c: bool) {
    let x = &s;
    let mut r = x;
    if c {
        r = &t;
    } else {
        println!("{}", r);
    }
}

fn copied_on_in_else(s: String, t: String, c: bool) {
    let x = &s;
    let mut r = x;
    if c {
        r = &t;
    } else {
        let r3 = r;
    }
}

fn same_loan_again(s: String) {
    let x = &s;
    let mut r = x;
    let n = r.len();
    if n > 0 {
        r = x;
    } else {
        let r3 = r;
    }
}

fn read_in_first_arm(s: String, t: String, n: i32) {
    let x = &s;
    let mut r = x;
    match n {
        0 => {
            println!("{}", r);
        }
        _ => {
            r = &t;
        }
    }
}

fn main() {}

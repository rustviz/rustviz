// A `&mut` reference a `for` loop's head moves into the loop's iterator
// (`zip(b)`, `chain(b)`, `for y in b`): its column ends at the move. The
// write through the loop variable and the loan's end belong to the
// iterator (and to the lender, when it has a column), not to b (#1365).
fn zipped(a: &mut Vec<i32>, b: &mut Vec<i32>) {
    for (x, y) in a.iter_mut().zip(b) {
        *y += *x;
    }
}

fn head(b: &mut Vec<i32>) {
    for y in b {
        *y += 1;
    }
}

fn chained(a: &mut Vec<i32>, b: &mut Vec<i32>) {
    for y in a.iter_mut().chain(b) {
        *y += 1;
    }
}

// The lender has a column: w stays borrowed until the loop ends.
fn local(v: Vec<i32>, a: &mut Vec<i32>) {
    let mut w = v;
    let b = &mut w;
    for (x, y) in a.iter_mut().zip(b) {
        *y += *x;
    }
    println!("{:?}", w);
}

fn main() {
    let mut a = vec![1];
    let mut b = vec![2];
    zipped(&mut a, &mut b);
    head(&mut b);
    chained(&mut a, &mut b);
    local(vec![3], &mut a);
}

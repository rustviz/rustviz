// #1163: the else branch of a let-chain that lent `*r` to bump. What r
// points to is an i32: a value, not a resource.
fn bump(r: &mut i32) -> bool { *r += 1; *r > 2 }

// The else branch of a let-chain that lent `*r` (an i32) to bump.
fn chain(r: &mut i32, o: Option<i32>) {
    if let Some(x) = o && bump(r) && x > 1 {
        println!("{}", x);
    } else {
        println!("no");
    }
}

fn main() {
    let mut k = 0;
    chain(&mut k, Some(3));
}

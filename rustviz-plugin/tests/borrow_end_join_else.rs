// The reassignment is in the `then` branch and the old reference is read
// in the `else` branch (#778): `x` stays borrowed until that read, the
// latest end over the two paths; `x += 1;` after the `if` is fine.
fn main() {
    let mut x = 8;
    let y = 9;
    let mut c = &x;
    let flag = true;
    if flag {
        c = &y;
    } else {
        println!("{}", c);
    }
    x += 1;
}

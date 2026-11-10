// `c` is read in the `if` arm and re-pointed in the `else` arm (#801): `x`
// stays borrowed in the `if` arm until that read (c's last use there), and
// in the `else` arm until `c = &y;`.
fn main() {
    let y = 9;
    let x = 8;
    let mut c = &x;
    let flag = true;
    if flag {
        println!("{}", c);
    } else {
        c = &y;
    }
}

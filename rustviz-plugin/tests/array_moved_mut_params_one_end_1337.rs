// `let k = [a, b, c];` with `&mut` parameters: a is moved into k, b and c
// are reborrowed. At k's last use its borrow ends once, on the dot its
// returns to b and c leave from. The end of the caller's loan k took from
// a used to be a second "k's mutable borrow ends" dot, on the row below,
// whenever two or more returns were stacked on that line (#1337).
fn three(a: &mut String, b: &mut String, c: &mut String) {
    let k = [a, b, c];
    k[2].push('!');
}
fn four(a: &mut String, b: &mut String, c: &mut String, d: &mut String) {
    let k = [a, b, c, d];
    k[3].push('!');
}
fn in_branch(a: &mut String, b: &mut String, c: &mut String, f: bool) {
    let k = [a, b, c];
    if f {
        k[2].push('!');
    }
}
fn main() {
    let (mut x, mut y, mut z, mut w) = (String::new(), String::new(), String::new(), String::new());
    three(&mut x, &mut y, &mut z);
    four(&mut x, &mut y, &mut z, &mut w);
    in_branch(&mut x, &mut y, &mut z, true);
}

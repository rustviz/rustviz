// #389 in a `match` arm: `v` gets its value back on the `_` arm's
// column after `r.push(2)`, not on its parent column.
fn main() {
    let mut v = vec![1];
    let n = 2;
    match n {
        0 => {
            println!("zero");
        }
        _ => {
            let r = &mut v;
            r.push(2);
            println!("pushed");
        }
    }
    println!("{:?}", v);
}

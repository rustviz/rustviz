// #425: a `let` inside a `match` arm exists only in that arm. It gets
// one plain column there, not a branched column across the whole `match`.
fn main() {
    let v = vec![1, 2];
    let n = 2;
    match n {
        0 => {
            println!("zero");
        }
        _ => {
            let first = &v[0];
            println!("{}", first);
        }
    }
}

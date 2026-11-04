// A match arm that returns (#363): `v` is still owned there and is
// dropped at the `return`; `w` was moved into `keep` first, so there's
// nothing left to drop for it.

fn keep(_w: Vec<i32>) {}

fn pick(n: i32) -> i32 {
    let v = vec![1, 2, 3];
    let w = vec![4];
    match n {
        0 => {
            keep(w);
            return -1;
        }
        _ => {
            println!("{:?} {:?}", v, w);
        }
    }
    n
}

fn main() {
    println!("{}", pick(1));
}

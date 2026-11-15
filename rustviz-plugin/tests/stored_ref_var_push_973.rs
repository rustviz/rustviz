// #973: `refs.push(&x)` with x a reference variable and refs a
// `Vec<&&String>` stores a borrow of x itself (a value), not of what x
// points to: with `let mut x`, rustc rejects `x = &b;` before
// `refs.len()`. Same for `&mut m`. A call that borrows x while refs
// holds it gives x back to the call only: refs still borrows x.
fn look(r: &&String) -> usize {
    r.len()
}

fn main() {
    let a = String::from("a");
    let x = &a;
    let mut refs = Vec::new();
    refs.push(&x);
    let n = look(&x);
    println!("{} {}", refs.len(), n);

    let mut b = String::from("b");
    let mut m = &mut b;
    let mut muts = Vec::new();
    muts.push(&mut m);
    println!("{}", muts.len());
    m.push('!');
}

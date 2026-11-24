// #1399, by reference: `it = refs.iter()` on a `Vec<&String>` borrows
// refs; the `&String`s in refs borrow a1, which it reaches only through
// refs. A reference taken out of it (`it.next().unwrap()`, a `&&String`)
// points into refs; one copied out of it (`it.next().copied()`,
// `it.copied().next()`, a `&String`) is the reference stored in refs,
// which borrows a1, not refs. All of them were drawn borrowing both
// ("so it may borrow from either").
fn main() {
    let a1 = String::from("a1");
    let refs = vec![&a1];
    let mut it = refs.iter();
    let r = it.next().unwrap();
    let it2 = refs.iter();
    let c = it2.copied().next().unwrap();
    let mut it3 = refs.iter();
    let d = it3.next().copied().unwrap();
    let mut it4 = refs.iter().copied();
    let q = it4.next().unwrap();
    println!("{} {} {} {} {:?}", r, c, d, q, refs);
}

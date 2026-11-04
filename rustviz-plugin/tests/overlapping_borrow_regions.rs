// #211: three shared borrows of x whose live ranges chain together
// (a: lines 9-11, c: 10-13, b: 12-14), so x stays borrowed from line 9
// to line 14 and only the last one to end (b) gives the borrow back.
// Grouping the borrows into regions used to depend on HashSet
// iteration order: on some runs c also "returned" the borrow on line 13
// while b still held it.
fn main() {
    let x = 5;
    let a = &x;
    let c = &x;
    println!("{}", a);
    let b = &x;
    println!("{}", c);
    println!("{}", b);
}

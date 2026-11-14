// #885: a `for` loop whose pattern copies each element out (`&y`,
// `&(a, b)`) or binds nothing (`_`) still borrows the collection: the
// loop's iterator holds the borrow until the loop ends, so `v.push(..)`
// anywhere in the body is E0502. Nothing on v's column said so.
fn main() {
    let v = vec![1, 2, 3];
    for &y in &v {
        println!("{}", y);
    }
    let pairs = vec![(1, 2), (3, 4)];
    for &(a, b) in &pairs {
        println!("{} {}", a, b);
    }
    let nums = vec![4, 5];
    for &n in nums.iter() {
        println!("{}", n);
    }
    let marks = vec![6, 7];
    for _ in &marks {
        println!("tick");
    }
    let mut w = vec![8, 9];
    for &mut x in &mut w {
        println!("{}", x);
    }
    println!("{} {} {} {} {}", v.len(), pairs.len(), nums.len(), marks.len(), w.len());
}

// #888: `for &w in &words` with words: Vec<&str> copies the `&str`
// stored in each element into w, so w points at the string literal, not
// into words: keeping w past the loop and then `words.push(..)` compiles.
// What borrows words is the loop's iterator, until the loop ends
// (`words.push(..)` in the body is E0502). w used to be drawn borrowing
// words. `&ref s` does borrow the element, so s still borrows names.
fn main() {
    let words = vec!["a", "b"];
    for &w in &words {
        println!("{}", w);
    }
    let pairs = vec![("x", 1), ("y", 2)];
    for &(k, n) in &pairs {
        println!("{} {}", k, n);
    }
    let mut tags = vec!["t"];
    for &mut t in &mut tags {
        println!("{}", t);
    }
    let names = vec![String::from("c")];
    for &ref s in &names {
        println!("{}", s);
    }
    println!("{} {} {} {}", words.len(), pairs.len(), tags.len(), names.len());
}

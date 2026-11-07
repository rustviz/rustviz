// #439: the literal a `let` pattern is matched against is a temporary
// with a `(temporary N)` column when it owns something to drop: dropped
// at the end of the statement, or with a `ref` binding at the end of
// the block it is in. A literal of Copy values has nothing to drop.
fn main() {
    let a = String::from("a");
    let b = String::from("b");
    {
        let (x, _) = (a, b);
        println!("{}", x);
    }
    let c = String::from("c");
    let d = String::from("d");
    {
        let (_, ref y) = (c, d);
        println!("{}", y);
    }
    let i = 1;
    let j = 2;
    let (_, ref k) = (i, j);
    println!("{} {}", k, j);
}

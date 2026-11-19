// #135: an async fn's body, and an async block's, aren't drawn: they run
// when the future is awaited. The parameters' dots of an async fn, and
// the first dot of a variable holding a future, say so. A plain fn's
// parameter gets no note.
async fn get(s: String) -> usize {
    s.len()
}

fn plain(t: String) -> usize {
    t.len()
}

fn main() {
    let a = String::from("a");
    let fut = get(a);
    let block = async { 1 };
    let n = plain(String::from("b"));
    drop(fut);
    drop(block);
    println!("{}", n);
}

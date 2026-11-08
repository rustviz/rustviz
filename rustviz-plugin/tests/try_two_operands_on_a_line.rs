// `let n = a? + b?;`: two `?` on one line, each on its own owner. a's `?`
// runs first; if it returns early, b (not yet touched) is dropped with
// the rest of the function's owned values. b's own `?` then moves b (#668).

fn add(a: Result<usize, String>, b: Result<usize, String>) -> Result<usize, String> {
    let n = a? + b?;
    Ok(n)
}

fn main() {
    let _ = add(Ok(1), Ok(2));
}

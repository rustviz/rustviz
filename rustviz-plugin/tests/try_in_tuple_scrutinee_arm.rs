// A `?` inside the arm of `match (x, y)` (or the branch of an
// `if let … = (x, y)`) returns from the function on `Err`, and that drops
// the temporary tuple too, with what of y's value is still in it (#653).
// Only on that path: the tuple is otherwise dropped where the match or
// the if branch ends.

fn in_arm(x: Option<String>, y: Option<String>, r: Result<i32, String>) -> Result<usize, String> {
    match (x, y) {
        (Some(a), _) => {
            let n = r?;
            Ok(a.len() + n as usize)
        }
        _ => Ok(0),
    }
}

fn in_if_let(p: Option<String>, q: Option<String>, w: Result<i32, String>) -> Result<usize, String> {
    if let (Some(b), _) = (p, q) {
        let k = w?;
        return Ok(b.len() + k as usize);
    }
    Ok(0)
}

fn main() {
    let _ = in_arm(Some(String::from("a")), None, Ok(1));
    let _ = in_if_let(Some(String::from("b")), None, Err(String::from("e")));
}

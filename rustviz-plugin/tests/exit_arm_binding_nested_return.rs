// #651: a `return` inside an `if` inside a match arm drops the arm's
// pattern binding `a` there, before anything declared outside the match.
fn plain(x: Option<String>, c: bool) -> usize {
    let t = String::from("t");
    match x {
        Some(a) => {
            if c {
                return 0;
            } else {
                println!("{}", a);
            }
            a.len() + t.len()
        }
        None => 1,
    }
}

fn main() {
    println!("{}", plain(Some(String::from("a")), true));
}

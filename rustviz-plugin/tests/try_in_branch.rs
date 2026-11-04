// `?` inside a branch: everything the function owns at that point is
// dropped on the error path, including the branch's own `u` (#430).

fn first_len(v: Vec<String>, flag: bool) -> Option<usize> {
    let t = String::from("t");
    if flag {
        let u = String::from("u");
        let s = v.first()?;
        return Some(s.len() + u.len());
    }
    Some(t.len())
}

fn main() {
    println!("{:?}", first_len(vec![String::from("a")], true));
}

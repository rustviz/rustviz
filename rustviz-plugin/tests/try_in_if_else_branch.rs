// `?` inside an `if` / `else` branch (#430): the dots saying what the
// `?` drops if it returns early go on the branch's columns, for the
// variables declared outside (`t`, `v`) and inside (`u`) the branch.
// They used to be lost once the `if` was drawn with columns.

fn first_len(v: Vec<String>, flag: bool) -> Option<usize> {
    let t = String::from("t");
    if flag {
        let u = String::from("u");
        let s = v.first()?;
        return Some(s.len() + u.len());
    } else {
        println!("no");
    }
    Some(t.len())
}

fn main() {
    println!("{:?}", first_len(vec![String::from("a")], true));
}

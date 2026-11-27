fn first<'a>(s: &'a String) -> &'a str {
    s.as_str()
}

fn main() {
    let s = String::from("ab");
    let s2 = s;
    let v = vec![s2, String::from("ab")];
    if v.len() > 1 && let Some(x) = v.first() {
        let s3 = 'blk: {
            String::from("hey")
        };
    }
}

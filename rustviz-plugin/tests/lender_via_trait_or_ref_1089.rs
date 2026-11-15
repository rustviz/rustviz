// #1089: a result carrying a reference stored in what an argument
// points to borrows that reference's lender, also when the lifetime
// comes from a trait (`Top<'a>`, implemented for `Vec<&'a str>`) or
// the argument is a reference variable (`rr = &rows`).
trait Top<'a> {
    fn top(&self) -> &'a str;
}

impl<'a> Top<'a> for Vec<&'a str> {
    fn top(&self) -> &'a str {
        self[0]
    }
}

fn inner<'a>(rows: &Vec<&'a Vec<i32>>) -> &'a i32 {
    &rows[0][1]
}

fn main() {
    let s = String::from("hi");
    let mut names = vec![s.as_str()];
    let t = names.top();
    names.push("x");
    println!("{}", t);
    let row = vec![1, 2];
    let rows = vec![&row];
    let rr = &rows;
    let h = inner(&*rr);
    let k = inner(rr);
    println!("{} {}", h, k);
}

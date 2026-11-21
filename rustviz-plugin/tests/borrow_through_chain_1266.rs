// #1266: a borrow through a method chain names the call that borrows
// (`last_mut`, `as_str`, `iter`), not the last call (`unwrap`, `trim`),
// which only passes the borrow on.
fn main() {
    let mut w = vec![1, 2];
    let xw: &mut i32 = w.last_mut().unwrap();
    *xw += 1;
    let s = String::from(" hi ");
    let t: &str = s.as_str().trim();
    let v = vec![3, 1, 2];
    let m: &i32 = v.iter().max().unwrap();
    let mut o: Option<String> = Some(String::from("o"));
    let r: &mut String = o.as_mut().unwrap();
    r.push('!');
    println!("{} {}", t, m);
}

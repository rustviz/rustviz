// #687: in a method chain, the value each call after the first works on
// is a temporary with no column. The variable's dot at the first call
// says so. No note where that value is a plain Copy value
// (`s.len().max(1)`), where it is drawn as a temporary column of its own
// (`cell.borrow().len()`), or where there is no chain (`v.len()`).
use std::cell::RefCell;

fn main() {
    let v = vec![1, 2, 3];
    let w: Vec<i32> = v.iter().map(|x| x + 1).collect();
    let s = String::from(" hi ");
    let t = s.trim().to_string();
    let n = s.len().max(1);
    let cell = RefCell::new(vec![1]);
    let k = cell.borrow().len();
    let len = v.len();
    println!("{:?} {} {} {} {}", w, t, n, k, len);
}

// #687: in a method chain, the value each call after the first works on
// is a temporary with no column. That needs no note: such a temporary
// is dropped by the end of the statement, so the figure isn't wrong
// about any variable. No chain here gets one; `cell.borrow().len()`
// keeps RefCell's run-time borrow note.
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

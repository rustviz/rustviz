// #1621: the `&mut v` rustc inserts for a method's receiver is a
// two-phase borrow. It's only reserved where it's written: the arguments
// run first (`v.len()` takes and gives back its shared borrow), and the
// mutable borrow starts when push is called.
struct Stack {
    v: Vec<usize>,
}

fn count(v: &Vec<usize>) -> usize {
    v.len()
}

fn main() {
    let mut v = Vec::new();
    v.push(v.len());
    let mut w = Vec::new();
    w.push(count(&w));
    let mut s = Stack { v: Vec::new() };
    s.v.push(s.v.len());
    let mut u = Vec::new();
    u.push(if u.is_empty() { 0 } else { u.len() });
    for _ in 0..2 {
        v.push(v.len());
    }
    println!("{:?} {:?} {:?} {:?}", v, w, s.v, u);
}

// An element returned by reference through a reference parameter (#1071):
// `&mut v[0]` is a reborrow of one element through `v` (via `IndexMut`),
// not `v` itself going to the caller. `v` isn't moved; the caller gets a
// reference to the element, and the Vec `v` points to stays borrowed
// while that reference is used.
fn first_mut(v: &mut Vec<i32>) -> &mut i32 {
    &mut v[0]
}

fn first(v: &Vec<i32>) -> &i32 {
    &v[0]
}

fn at(xs: &[i32], i: usize) -> &i32 {
    &xs[i]
}

fn cell_mut(g: &mut Vec<Vec<i32>>, i: usize, j: usize) -> &mut i32 {
    &mut g[i][j]
}

fn last_mut(w: &mut Vec<String>) -> &mut String {
    let n = w.len() - 1;
    return &mut w[n];
}

fn main() {
    let mut v = vec![1, 2, 3];
    let a = first_mut(&mut v);
    *a += 1;
    let b = first(&v);
    let c = at(&v, 1);
    println!("{} {}", b, c);
    let mut g = vec![vec![0; 2]; 2];
    let d = cell_mut(&mut g, 1, 0);
    *d = 5;
    let mut w = vec![String::from("x")];
    let e = last_mut(&mut w);
    e.push('!');
    println!("{:?} {:?} {:?}", v, g, w);
}

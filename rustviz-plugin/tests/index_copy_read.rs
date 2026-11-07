// Copying an element out of an index (#297): `let first = v[0];`
// copies one i32 out of v's buffer, and v keeps all of it. (Moving a
// String out of `v[0]` doesn't compile: E0507.)

fn head(xs: &[i32]) -> i32 {
    let h = xs[0];
    h
}

fn main() {
    let mut v = vec![1, 2, 3];
    let first = v[0];
    v.push(4);
    let grid = [[1, 2], [3, 4]];
    let cell = grid[1][0];
    println!("{} {} {}", first, cell, head(&v));
}

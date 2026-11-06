// Writes to an element (`v[i] = …`) are drawn on the collection's
// column: the collection keeps its resource and one element of it is
// overwritten. A value moved into an element (`names[0] = s`, and a
// struct's fields with `ps[0] = p`) leaves its old owner. (#144)
struct P {
    name: String,
}

fn main() {
    let mut v = vec![1, 2, 3];
    let k = 5;
    v[0] = 9;
    v[1] += k;
    v[2] = v[0];
    let z = &k;
    v[0] = *z;
    let mut names = vec![String::new()];
    let s = String::from("x");
    names[0] = s;
    names[0] = String::from("y");
    names[0] += "!";
    let mut grid = [[0; 2]; 2];
    grid[1][0] = 7;
    let y = 3;
    grid[0][1] = y;
    grid[1][1] += 1;
    let mut cube = vec![vec![vec![1, 2]]];
    cube[0][0][1] = 5;
    let mut rows = vec![vec![String::new()]];
    rows[0][0] = String::from("q");
    let mut ps = vec![P { name: String::new() }];
    let p = P { name: String::from("z") };
    ps[0] = p;
    println!("{:?} {:?} {:?} {:?} {:?} {}", v, names, grid, cube, rows, ps[0].name);
}

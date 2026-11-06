// #316: a Copy variable bound in every arm holds a value; it doesn't
// "acquire ownership of a resource" at the merge.
#[derive(Clone, Copy)]
struct Point {
    x: i32,
    y: i32,
}

fn main() {
    let x = 3;
    let n = match x {
        0 => 1,
        _ => 2,
    };
    let c = n > 1;
    let p = if c {
        Point { x: 1, y: 2 }
    } else {
        Point { x: 3, y: 4 }
    };
    let s = if c {
        String::from("a")
    } else {
        String::from("b")
    };
    println!("{} {} {} {}", n, p.x, p.y, s);
}

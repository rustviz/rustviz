struct Point { x: i32, y: i32 }

fn main() {
    let x = 1;
    let y = 2;
    let z = x + y;
    let a = Point { x: 1, y: 2 };
    let c = a.x + a.y;
    let d = a.x;
    let w = x * 2 - 1;
    let bigger = x < y;
    println!("{} {} {} {} {}", z, c, d, w, bigger);
}

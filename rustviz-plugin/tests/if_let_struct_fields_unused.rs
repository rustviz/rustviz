// A struct bound by a pattern whose fields are never used, so none of
// its fields gets a column (#192). Its struct box must wrap its own
// label; it used to be drawn with width="-80".
struct Point {
    x: i32,
    y: i32,
}

fn main() {
    let o = Some(Point { x: 1, y: 2 });
    if let Some(p) = o {
        println!("got a point");
    }
}

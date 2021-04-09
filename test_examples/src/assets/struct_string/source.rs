struct Foo {
    x: i32,
    y: String,
}

fn main() {
    let _y = String :: from("bar");
    let f = Foo { x: 5, y: _y };
    println!("{}", f.x);
    println!("{}", f.y);
}
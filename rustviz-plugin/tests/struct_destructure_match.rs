struct Inner { x: String, y: i32 }
struct Outer { inner: Inner, tag: String }

fn main() {
    let o = Outer { inner: Inner { x: String::from("x"), y: 1 }, tag: String::from("t") };
    match o {
        Outer { inner: Inner { x, y }, tag } => println!("{} {} {}", x, y, tag),
    }
}

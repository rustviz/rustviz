macro_rules! show {
    ($e:expr) => {
        println!("{}", $e)
    };
}
macro_rules! show_block {
    ($e:expr) => {{
        println!("{:?}", $e);
    }};
}
macro_rules! five {
    () => {
        5
    };
}
fn main() {
    let w = String::from("hi");
    let v = vec![1, 2];
    let n = five!();
    show!(w);
    show_block!(v);
    println!("{}", n);
}

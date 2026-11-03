fn apply<F: Fn(i32) -> i32>(f: F) -> i32 {
    f(1)
}

fn main() {
    let k = 10;
    let r = apply(|x| x + k);
    println!("{}", r);
}

fn apply<F: Fn(i32) -> i32>(f: F) -> i32 {
    let r = f(1);
    r
}

fn main() {
    let add_one = |x: i32| x + 1;
    let y = add_one(5);
    let s = String::from("hi");
    let make = |n: usize| s.repeat(n);
    let t = make(2);
    let z = apply(|x| x * 2);
    println!("{} {} {} {}", y, s, t, z);
}

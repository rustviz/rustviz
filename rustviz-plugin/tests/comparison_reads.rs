fn take(_k: i32) {}

fn main() {
    let t = 1;
    let lim = 5;
    let z = t < lim;
    let w = t + lim;
    if t == lim {
        take(t);
    }
    let big = t >= 3;
    println!("{} {} {}", z, w, big);
}

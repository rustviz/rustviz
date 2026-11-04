// Array literals (#297). The array is one value with its own column;
// a variable written as an element is moved into it (copied if its
// type is Copy). A struct moved into an array takes its fields along.

struct P {
    name: String,
}

fn main() {
    let s = String::from("a");
    let t = String::from("b");
    let a = [s, t];
    let x = 1;
    let nums = [x, 2, 3];
    let zeros = [0; 4];
    let p = P { name: String::from("p") };
    let ps = [p];
    println!("{:?} {:?} {:?} {}", a, nums, zeros, ps[0].name);
}

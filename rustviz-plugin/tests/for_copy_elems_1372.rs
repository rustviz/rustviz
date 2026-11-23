// #1372: `for y in ys` with Copy elements moves ys into the loop's
// iterator; each y is a copy of an element. The arrow out of ys says
// so, as `for s in v` draws "Move of each element of v to s".
fn total(ys: Vec<i32>) -> i32 {
    let mut t = 0;
    for y in ys {
        t += y;
    }
    t
}

fn main() {
    let ys = vec![1, 2, 3];
    for y in ys {
        if y > 1 {
            println!("{}", y);
        }
    }
    let mut a = String::from("a");
    let mut b = String::from("b");
    let refs = vec![&mut a, &mut b];
    for r in refs {
        r.push('!');
    }
    println!("{} {} {}", total(vec![4]), a, b);
}

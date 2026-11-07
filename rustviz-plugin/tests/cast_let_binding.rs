fn count(v: &Vec<i32>) -> i32 {
    let n = v.len() as i32;
    let m = v.len();
    if n > 2 {
        return n;
    }
    m as i32
}

fn main() {
    let v = vec![1, 2, 3];
    let total = count(&v);
    let a = 3;
    let b = a as i64;
    let c = (b + 1) as i32;
    let d = &a;
    let e = *d as u8;
    println!("{} {} {} {}", total, b, c, e);
}

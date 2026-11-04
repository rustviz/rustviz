// Element writes through a reference (`xs[i] = x` with
// `xs: &mut [i32]`, `r[0] = …` with `r = &mut v`) modify the resource
// the reference points to; the reference doesn't own anything. The
// borrow of a reference parameter still ends on the write's line.
// (#144)
fn set_all(xs: &mut [i32], x: i32) {
    for i in 0..xs.len() {
        xs[i] = x;
    }
}

fn put(dst: &mut Vec<String>, s: String) {
    dst[0] = s;
}

fn main() {
    let mut a = [0; 3];
    set_all(&mut a, 4);
    let mut v = vec![String::new()];
    put(&mut v, String::from("x"));
    let r = &mut v;
    r[0] = String::from("y");
    println!("{:?} {:?}", a, v);
}

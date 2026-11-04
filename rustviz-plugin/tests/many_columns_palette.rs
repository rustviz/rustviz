// More variables and calls than the 12-colour palette (#279): the
// later columns used to get no stroke, so their state lines vanished.
fn main() {
    let v = vec![1, 2, 3];
    let r = &v;
    let n = r.into_iter().count();
    let k = r.iter().map(|x| x * 2).sum::<i32>();
    let s = String::from("a b");
    let rs = &s;
    let w = rs.split(' ').count();
    let m = v.len();
    let t = String::from("c");
    let u = t;
    let a = 1;
    let b = a + 1;
    let c = b * 2;
    let d = c - a;
    println!("{} {} {} {} {} {}", n, k, w, m, u, d);
}

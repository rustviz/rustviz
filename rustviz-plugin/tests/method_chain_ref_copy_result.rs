fn main() {
    let v = vec![1, 2, 3];
    let r = &v;
    let n = r.into_iter().count();
    let k = r.iter().map(|x| x * 2).sum::<i32>();
    let s = String::from("a b");
    let rs = &s;
    let w = rs.split(' ').count();
    let m = v.len();
    println!("{} {} {} {}", n, k, w, m);
}

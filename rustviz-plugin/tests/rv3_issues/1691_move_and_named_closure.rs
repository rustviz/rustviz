fn main() {
    let a = 1;
    let mut b = 2;
    let p = &b;
    let o = Some(&a).filter(|_| false);
    let r = o.unwrap_or_else(move || p);
    let f = || &b;
    let r2 = o.unwrap_or_else(f);
    println!("{r} {r2}");
}

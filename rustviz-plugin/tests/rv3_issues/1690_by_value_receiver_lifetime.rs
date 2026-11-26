fn main() {
    let a = 1;
    let b = 2;
    let r;
    {
        let o = Some(&a).filter(|_| false);
        r = o.unwrap_or_else(|| &b);
    }
    println!("{r}");
}

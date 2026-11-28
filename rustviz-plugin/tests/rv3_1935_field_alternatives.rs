struct Holder<'a> {
    r: &'a String,
    n: i32,
}

fn run(r: &String, s: String, o: Option<String>, flag: bool) -> usize {
    let q = if flag { r } else { &s };
    let h = Holder { r: q, n: 1 };
    let Some(x) = o else {
        println!("{} {r}", h.n);
        return 0;
    };
    println!("{r}");
    x.len()
}

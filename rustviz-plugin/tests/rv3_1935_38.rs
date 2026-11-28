struct Holder<'a> {
    r: &'a String,
    n: i32,
}

fn run(r: &String, o: Option<String>) -> Option<usize> {
    let h = Holder { r: r, n: 1 };
    let Some(x) = o else {
        println!("{}", h.n);
        return None;
    };
    Some(0)
}

fn run(s: String, take: bool, consume: bool) {
    let c = move || s;
    if take {
        if consume { drop(c()); }
        let n = {
            1
        };
        println!("{n}");
    }
}

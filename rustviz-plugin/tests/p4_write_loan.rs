fn writes(cond: bool) {
    {
        let mut x = 1;
        let y = 2;
        let mut z = &x;
        if cond {
            x = 2;
            z = &y;
        }
        println!("{} {}", z, x);
    }
    { let x = 3; drop(x); }
}

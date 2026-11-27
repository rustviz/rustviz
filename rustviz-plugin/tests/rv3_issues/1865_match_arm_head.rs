fn run(o: Option<i32>) {
    let mut s = String::from("x");
    let x = &s;
    match o {
        Some(_) => {
            let m = &mut s;
            m.push('a');
        }
        None => {
            println!("None");
            println!("{}", x);
        }
    }
}

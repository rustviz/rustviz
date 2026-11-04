fn main() {
    let opt = Some(String::from("hi"));
    match opt {
        Some(ref s) => println!("{}", s),
        None => {}
    }
    match &opt {
        Some(t) => println!("{}", t),
        None => {}
    }
    println!("{:?}", opt);
}

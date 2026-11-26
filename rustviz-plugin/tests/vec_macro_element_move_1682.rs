fn main() {
    let r = String::from("r");
    let s2 = String::from("ab");
    let v = vec![s2, format!("{}!", r)];
    println!("{}", v.len());
}

fn assignment() {
    let prefix = String::from("r");
    let moved = String::from("ab");
    let v;
    v = vec![format!("{}!", prefix), moved];
    println!("{}", v.len());
}

fn argument() {
    let moved_arg = String::from("ab");
    consume(vec![moved_arg, format!("ab")]);
}

fn consume(v: Vec<String>) {
    println!("{}", v.len());
}

fn main() {
    let opt = Some(String::from("hi"));
    match opt {
        Some(ref s) => println!("{}", s),
        None => {}
    }
    let mut name = Some(String::from("ann"));
    if let Some(ref mut n) = name {
        n.push('!');
    }
    let num = Some(5);
    match num {
        Some(ref k) => println!("{}", k),
        None => {}
    }
    println!("{:?} {:?} {:?}", opt, name, num);
}

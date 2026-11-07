fn main() {
    let c = true;
    let s = String::from("a");
    let v = vec![1, 2];
    if c {
        if !c {
            let z = String::from("z");
            println!("{}", z);
        }
        for n in &v {
            println!("{}", n);
        }
        println!("{}", s);
    } else {
        drop(s);
    }
}

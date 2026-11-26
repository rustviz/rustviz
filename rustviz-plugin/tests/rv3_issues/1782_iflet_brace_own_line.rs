fn main() {
    let s = String::from("s");
    let v = vec![String::from("a"), String::from("b")];
    if let Some(x) = v.first()
    {
        if let Some(_y) = v.last() {
            let _t = s;
        }
        if x.is_empty() {
            println!("{}", x);
        }
    }
    println!("{}", v.len());
}

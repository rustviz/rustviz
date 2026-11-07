fn main() {
    let mut s = String::from("hello");
    let o = Some(&mut s);
    if let Some(x) = o {
        x.push('a');
        println!("{}", x);
    }
}

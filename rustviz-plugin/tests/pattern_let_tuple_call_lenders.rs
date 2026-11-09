fn two<'a, 'b>(a: &'a String, b: &'b String) -> (&'a String, &'b String) {
    (a, b)
}
fn main() {
    let c = String::from("c");
    let mut d = String::from("d");
    let (x, y) = two(&c, &d);
    println!("{}", y);
    d.push('!');
    println!("{}", x);
}

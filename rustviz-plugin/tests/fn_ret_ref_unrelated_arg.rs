fn pick_first<'a, 'b>(x: &'a str, _y: &'b str) -> &'a str {
    x
}

fn main() {
    let a = String::from("a");
    let b = String::from("b");
    let r = pick_first(&a, &b);
    println!("{}", r);
}

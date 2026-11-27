fn id(value: &String) -> &String { value }

fn example(a: String, b: String, d: String, change: bool) {
    let mut r = id(&a);
    if change {
        r = &b;
    }
    println!("{}", r);
    r = &d;
    println!("{}", r);
}

fn main() {}

enum E {
    Two(String, String),
}

fn early(x: Option<String>, y: Option<String>) -> usize {
    match (x, y) {
        (Some(a), _) => {
            println!("{}", a);
            a.len()
        }
        _ => return 0,
    }
}

fn early_if(p: Option<String>, q: Option<String>) -> i32 {
    if let (Some(_), Some(r)) = (p, q) {
        println!("{}", r);
        return 1;
    }
    0
}

fn main() {
    let x = Some(String::from("a"));
    let y = Some(String::from("b"));
    match (x, y) {
        _ => println!("none"),
    }

    let e = E::Two(String::from("c"), String::from("d"));
    let f = Some(String::from("f"));
    if let (E::Two(c, _), Some(g)) = (e, f) {
        println!("{} {}", c, g);
    }
    println!("{} {}", early(None, None), early_if(None, None));
}

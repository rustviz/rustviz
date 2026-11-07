fn nested_return(x: Option<String>, y: Option<String>, c: bool) -> usize {
    match (x, y) {
        (Some(a), _) => {
            if c {
                return 0;
            } else {
                println!("{}", a);
            }
            a.len()
        }
        _ => 1,
    }
}

fn arm_returns(s: Option<String>, t: Option<String>) -> usize {
    match (s, t) {
        (Some(a), _) => a.len(),
        _ => {
            println!("none");
            return 2;
        }
    }
}

fn iflet_nested(p: Option<String>, q: Option<String>, c: bool) -> i32 {
    if let (Some(_), Some(r)) = (p, q) {
        if c {
            return 1;
        } else {
            println!("{}", r);
        }
    }
    0
}

fn exits_without_drops(m: Option<String>, n: Option<String>) {
    match (m, n) {
        (Some(a), _) => println!("{}", a),
        _ => std::process::exit(1),
    }
}

fn main() {
    println!("{}", nested_return(None, None, false));
    println!("{}", arm_returns(None, None));
    println!("{}", iflet_nested(None, None, false));
    exits_without_drops(Some(String::from("m")), None);
}

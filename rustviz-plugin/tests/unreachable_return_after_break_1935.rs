fn make() -> String {
    String::from("made")
}

fn id<T>(t: T) -> T {
    t
}

fn main() {
    let s = String::from("hi");
    let s2 = id(s);
    let v = vec![make(), String::from("x")];
    if v.len() > 1 {
    }
    let b = Box::new(s2);
    let c = || b.len();
    'blk2: {
        if true { break 'blk2; }
        return;
    }
}

struct Rect {
    w: u32,
}

fn main() {
    let s = String::from("hi");
    let n = 5;
    println!("{} {}", s, s);
    println!("{n}");
    let t = format!("{}!", s);
    eprintln!("{}", t);
    let rect = Rect { w: 30 };
    let r = &rect;
    println!("{}", r.w);
    let p = &n;
    println!("{}", *p);
    assert!(n > 0, "n was {}", t);
}

use std::fmt::Write;

fn main() {
    let s = String::from("a");
    let r1 = &s;
    let r2 = &s;
    println!("{}", r1);
    print!("{} {}", r1, r2);
    eprintln!("{}", r2);
    eprint!("{}", r1);
    let t = format!("{}-{}", r1, r2.len());
    let mut out = String::new();
    write!(out, "{}", r1).unwrap();
    writeln!(out, "{}", r2).unwrap();
    println!("{t}");
    if out.len() > 100 {
        panic!("bad {}", r1);
    }
}

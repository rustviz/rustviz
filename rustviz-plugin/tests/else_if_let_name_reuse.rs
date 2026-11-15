// #916: variables of one name declared in different arms of a
// conditional share a column. Each arm's borrow end used to be put in
// the first arm that declares the name, so the later arm's lender read
// "being shared" after the arms joined (and across `b = None;`,
// `d.push('!')`, which need it unborrowed).
fn main() {
    let a: Option<String> = None;
    let mut b = Some(String::from("b"));
    if let Some(x) = &a {
        println!("{}", x);
    } else if let Some(x) = &b {
        println!("{}", x);
    }
    b = None;
    println!("{:?} {:?}", a, b);
    let n = 3;
    let mut c = String::from("c");
    let mut d = String::from("d");
    if n > 2 {
        let r = &c;
        println!("{}", r);
    } else {
        let r = &d;
        println!("{}", r);
    }
    d.push('!');
    match n {
        0 => {
            let q = &c;
            println!("{}", q);
        }
        _ => {
            let q = &d;
            println!("{}", q);
        }
    }
    d.push('?');
    println!("{} {}", c, d);
}

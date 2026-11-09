// #762 follow-up: the Copy arrow from a call says "the value f() returns"
// only when the variable gets the whole result. When it gets part of it
// (inside an Option or Ok, a tuple field) the label mirrors the dot.

fn pair() -> (String, i32) { (String::from("p"), 7) }

fn parse_it() -> Result<i32, std::num::ParseIntError> {
    let n: i32 = "5".parse()?;
    Ok(n)
}

fn doubled() -> Result<i32, std::num::ParseIntError> {
    let d: i32 = "6".parse::<i32>().map(|x| x + 1)?;
    Ok(d)
}

fn main() {
    let mut v = vec![1, 2, 3];
    while let Some(t) = v.pop() {
        println!("{}", t);
    }
    let len = v.len();
    let k: i32 = 4;
    if let Some(m) = k.checked_mul(2) {
        println!("{}", m);
    }
    let words = vec![1, 2];
    if let Some(w) = words.last() {
        println!("{}", w);
    }
    let s;
    let n;
    (s, n) = pair();
    println!("{} {} {:?} {}", s, n, parse_it(), len);
    println!("{:?}", doubled());
}

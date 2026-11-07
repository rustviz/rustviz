use std::borrow::Cow;

enum Shape {
    Named(String),
    Empty,
}

enum Either {
    Left(String),
    Right(String),
}

enum Mixed {
    Text(String),
    Items(Vec<i32>),
}

enum Plain {
    A(i32),
    B,
}

fn main() {
    let a: Result<i32, String> = Err(String::from("a"));
    let b: Result<String, String> = Ok(String::from("b"));
    let c: Result<String, Vec<i32>> = Ok(String::from("c"));
    let d: Result<i32, i32> = Ok(1);
    let e = Shape::Named(String::from("e"));
    let f = Either::Left(String::from("f"));
    let g = Mixed::Text(String::from("g"));
    let h = Plain::A(1);
    let i: Cow<str> = Cow::Owned(String::from("i"));
    println!("{:?} {:?} {:?} {:?}", a, b, c, d);
    let _ = (&e, &f, &g, &h, &i);
}

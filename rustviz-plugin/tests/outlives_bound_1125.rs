fn shorten<'a, 'b>(x: &'a str, _y: &'b str) -> &'b str where 'a: 'b { x }
fn pick<'a, 'b: 'a>(x: &'a str, y: &'b str) -> &'a str { if x.len() > 3 { x } else { y } }
struct Part<'p> { text: &'p str }
fn part<'a, 'b>(x: &'a str, _y: &'b str) -> Part<'b> where 'a: 'b { Part { text: x } }
fn main() {
    let s1 = String::from("hello");
    let t1 = String::from("world");
    let r1 = shorten(&s1, &t1);
    let _spacer = 1;
    println!("{}", r1);
    let s2 = String::from("hello");
    let t2 = String::from("world");
    let r2 = pick(&s2, &t2);
    let _spacer2 = 1;
    println!("{}", r2);
    let s3 = String::from("hello");
    let t3 = String::from("world");
    let mut r3 = "x";
    println!("{}", r3);
    r3 = shorten(&s3, &t3);
    let _spacer3 = 1;
    println!("{}", r3);
    let s4 = String::from("hello");
    let t4 = String::from("world");
    let p4 = part(&s4, &t4);
    let _spacer4 = 1;
    println!("{}", p4.text);
}

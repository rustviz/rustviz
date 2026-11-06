struct W { s: String }
impl W {
    fn pick(&self, other: &str) -> &str {
        if other.len() > 1 { &self.s } else { &self.s }
    }
    fn pick_arg<'a>(&self, other: &'a str) -> &'a str {
        other
    }
    fn both<'a>(&'a self, other: &'a str) -> &'a str {
        if other.len() > 1 { &self.s } else { other }
    }
}
fn three<'a>(a: &'a str, b: &'a str, c: &'a str) -> &'a str {
    if a.len() > 1 { a } else if b.len() > 1 { b } else { c }
}
fn first_third<'a, 'b>(a: &'a str, _b: &'b str, c: &'a str) -> &'a str {
    if a.len() > 1 { a } else { c }
}
fn main() {
    let w = W { s: String::from("x") };
    let o = String::from("yy");
    let r1 = w.pick(&o);
    println!("{}", r1);
    let r2 = w.pick_arg(&o);
    println!("{}", r2);
    let r3 = w.both(&o);
    println!("{}", r3);
    let a = String::from("a");
    let b = String::from("b");
    let c = String::from("c");
    let r4 = three(&a, &b, &c);
    println!("{}", r4);
    let r5 = first_third(&a, &b, &c);
    println!("{}", r5);
    let r6 = three(&a, "lit", &c);
    println!("{}", r6);
}

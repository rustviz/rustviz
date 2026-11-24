// #1354: fields that may borrow from any of several lenders, beyond the
// two-field struct of `field_two_lenders_1354`: a tuple of `&mut`s, three
// fields from three lenders, and a field assigned a call's result. One
// arrow per field, naming every lender.
struct Three<'p> { a: &'p str, b: &'p str, c: &'p str }

fn three<'a>(x: &'a str, y: &'a str, z: &'a str) -> Three<'a> {
    Three { a: x, b: y, c: z }
}

fn pair<'a>(x: &'a mut String, y: &'a mut String) -> (&'a mut String, &'a mut String) {
    (x, y)
}

fn pick<'a>(x: &'a str, y: &'a str) -> &'a str {
    if x.len() > y.len() { x } else { y }
}

struct One<'p> { a: &'p str }

fn tuple_of_mut() {
    let mut s1 = String::from("hi");
    let mut s2 = String::from("yo");
    let t = pair(&mut s1, &mut s2);
    t.0.push('!');
    t.1.push('?');
}

fn three_lenders() {
    let s1 = String::from("hi");
    let s2 = String::from("yo");
    let s3 = String::from("ho");
    let d = three(&s1, &s2, &s3);
    println!("{} {} {}", d.a, d.b, d.c);
}

fn field_assigned_call() {
    let s1 = String::from("hi");
    let s2 = String::from("yo");
    let mut o = One { a: "" };
    o.a = pick(&s1, &s2);
    println!("{}", o.a);
}

fn main() {
    tuple_of_mut();
    three_lenders();
    field_assigned_call();
}

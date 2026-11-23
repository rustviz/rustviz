// #1345: `*b = u;` on a `Box<String>` drops the old String where the
// new one arrives: the dot gets the drop mark, as `v = w;` does. With
// a Box of a tuple drawn as its parts (#893), the write from a
// variable is one dot, as from a literal.
fn make() -> String {
    String::from("m")
}

fn main() {
    let mut b = Box::new(String::from("b"));
    let u = String::from("u");
    *b = u;
    let mut b3 = Box::new(String::from("c"));
    *b3 = make();
    let mut b2 = Box::new((String::from("d"), 2));
    let (s3, _) = *b2;
    let o = (String::from("o"), 5);
    *b2 = o;
    let mut b4 = Box::new((String::from("e"), 4));
    let o2 = (String::from("o"), 5);
    *b4 = o2;
    println!("{} {} {} {} {}", b, b3, s3, b2.0, b4.0);
}

// #1343: a swap or rotation by destructuring assignment reads every
// part before it assigns any, so each target's value moves out before
// its new one arrives: one dot per target, "moved" then "acquires".
struct P {
    name: String,
}

fn main() {
    let mut a = String::from("a");
    let mut b = String::from("b");
    (a, b) = (b, a);
    let mut c = String::from("c");
    (a, b, c) = (b, c, a);
    let mut p = P { name: String::from("p") };
    let mut q = P { name: String::from("q") };
    (p, q) = (q, p);
    let mut u = String::from("u");
    let mut w = String::from("w");
    let mut m1 = &mut u;
    let mut m2 = &mut w;
    (m1, m2) = (m2, m1);
    m1.push('!');
    m2.push('?');
    println!("{} {} {} {} {} {} {}", a, b, c, p.name, q.name, u, w);
}

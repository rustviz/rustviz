struct P { name: String, age: u32 }

fn main() {
    let s = String::from("hi");
    let ref r = s;
    let mut w = String::from("yo");
    let ref mut m = w;
    m.push('!');
    let p = P { name: String::from("p"), age: 1 };
    let P { ref name, .. } = p;
    let x = 5;
    let ref k = x;
    let ref t = String::from("tmp");
    let u = String::from("u");
    let (ref a, b) = (u, 1);
    println!("{} {} {} {} {} {} {} {} {}", r, s, w, name, p.age, k, t, a, b);
}

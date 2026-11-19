// A tuple or struct whose fields are moved out one by one: once one
// is, it can't be used as a whole (E0382, "use of partially moved
// value"); once all are, nothing is left in it and its line stops. Its
// line used to say "t is the owner of the resource" to the end (#1157).
struct P { a: String, b: String }
struct In { x: String, y: String }
struct Out { inner: In, tag: String }

fn tuple_all() {
    let s1 = String::from("a");
    let s2 = String::from("b");
    let t = (s1, s2);
    let (x, y) = t;
    println!("{} {}", x, y);
}

fn struct_one_by_one() {
    let p = P { a: String::from("a"), b: String::from("b") };
    let pa = p.a;
    println!("{}", pa);
    let pb = p.b;
    println!("{}", pb);
}

fn copy_part_stays() {
    let u = (String::from("u"), 5);
    let (u0, _) = u;
    println!("{} {}", u0, u.1);
}

fn nested() {
    let o = Out { inner: In { x: String::from("x"), y: String::from("y") }, tag: String::from("t") };
    let ox = o.inner.x;
    println!("{}", ox);
    let oy = o.inner.y;
    println!("{} {}", oy, o.tag);
}

fn in_branch(c: bool) {
    let q = P { a: String::from("a"), b: String::from("b") };
    if c {
        let qa = q.a;
        println!("{}", qa);
    }
    println!("{}", q.b);
}

fn refilled() {
    let mut r = P { a: String::from("a"), b: String::from("b") };
    let ra = r.a;
    r.a = ra;
    println!("{}", r.a);
}

fn main() {
    tuple_all();
    struct_one_by_one();
    copy_part_stays();
    nested();
    in_branch(true);
    refilled();
}

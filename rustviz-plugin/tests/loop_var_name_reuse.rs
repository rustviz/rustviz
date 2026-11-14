// #903: variables of one name that are never in scope together (two
// `for x` loops, sibling blocks, a `for y` then a `let y`, two `if let`s,
// two `while let`s) share a column, and the later one's borrow used to
// replace the earlier one's: the earlier borrow never ended, so its
// lender read "being shared" until some later event reset it.
fn main() {
    let v = vec![1, 2, 3];
    for x in v[..2].iter() {
        println!("{}", x);
    }
    let n = v.len();
    for (i, x) in v.iter().enumerate() {
        println!("{} {} {}", i, x, n);
    }
    let mut w = vec![1, 2, 3];
    {
        let r = &w;
        println!("{}", r[0]);
    }
    w.push(4);
    {
        let r = &w;
        println!("{}", r.len());
    }
    let mut u = vec![String::from("a")];
    for y in u.iter() {
        println!("{}", y);
    }
    u.push(String::from("b"));
    let y = &u;
    println!("{:?}", y);
    let mut o = Some(String::from("hi"));
    if let Some(s) = &o {
        println!("{}", s);
    }
    o = None;
    if let Some(s) = &o {
        println!("{}", s);
    }
    let mut t = vec![1, 2, 3];
    {
        let it = t.iter();
        println!("{}", it.count());
    }
    t.push(4);
    {
        let it = t.iter();
        println!("{}", it.count());
    }
    let mut q = vec![1, 2];
    let mut it1 = q.iter();
    while let Some(e) = it1.next() {
        println!("{}", e);
    }
    q.push(3);
    let mut it2 = q.iter();
    while let Some(e) = it2.next() {
        println!("{}", e);
    }
}

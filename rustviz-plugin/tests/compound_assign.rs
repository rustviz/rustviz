struct Acc {
    total: i32,
}

fn bump(n: &mut i32, by: i32) {
    *n += by;
}

fn first(v: &mut Vec<i32>, k: i32) {
    v[0] += k;
}

fn main() {
    let x = 2;
    let mut acc = 1;
    acc += x;
    let mut a = Acc { total: 0 };
    a.total += acc;
    bump(&mut acc, x);
    let mut s = String::from("a");
    let t = String::from("b");
    s += &t;
    s += "!";
    let r = &mut s;
    *r += "c";
    let mut v = vec![1];
    first(&mut v, x);
    println!("{} {} {} {}", acc, a.total, s, t);
}

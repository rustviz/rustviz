struct S { n: i32, a: String }
fn main() {
    let mut s = S { n: 1, a: String::new() };
    let flag = s.n > 0;
    let r1 = &mut s;
    if flag {
        r1.n = 3;
        r1.a.push('x');
    } else {
        r1.n = 4;
    }
    println!("{}", s.n);
}

fn matched(mut data: S, which: Option<i32>) {
    let r = &mut data;
    match which {
        Some(n) => {
            r.n = n;
            r.a.push('!');
        }
        None => r.n = 0,
    }
}

fn shared(data: S, flag: bool) {
    let q;
    if flag {
        q = &data;
        println!("{}", q.n);
    }
}

struct H<'a> { data: &'a mut String }
fn reference_field(mut h: H<'_>, flag: bool) {
    let r = &mut h;
    if flag {
        r.data.push('x');
        println!("{}", r.data);
    }
}

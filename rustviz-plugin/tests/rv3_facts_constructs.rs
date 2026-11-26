// RustViz 3 fact layer (rustviz-plugin/src/facts/): one of each construct
// P0 records, pinned by `facts_pin_constructs` in rustviz-lib/tests/corpus.rs.
struct H<'a> {
    r: &'a mut i32,
    n: i32,
}

struct S {
    a: String,
    b: String,
}

fn main() {
    let mut list = vec![1, 2, 3];
    let mut f = || list.push(4);
    f();
    println!("{list:?}");
    let mut n = 5;
    let h = H { r: &mut n, n: 1 };
    *h.r += h.n;
    let mut v = vec![1];
    v.push(v.len());
    let r = &mut v;
    r.push(2);
    let t = &String::from("tmp");
    println!("{}", t);
    let c = n > 3;
    if c { v.push(1); } else { drop(v); }
    let b = Box::new(S { a: String::new(), b: String::new() });
    let x = b.a;
    let o = Some(String::from("o"));
    if let Some(ref s) = o {
        println!("{s}");
    }
    let mut arr = [1, 2];
    let e = &mut arr[0];
    *e += 1;
    println!("{} {} {}", x, b.b, arr[1]);
}

fn early(c: bool) -> usize {
    let s = String::from("s");
    if c {
        return 0;
    }
    s.len()
}

fn moving_closure() -> usize {
    let s = String::from("s");
    let g = move || {
        let t = s;
        t.len()
    };
    g()
}

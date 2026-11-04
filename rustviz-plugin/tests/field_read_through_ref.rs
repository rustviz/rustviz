// Reads of a struct's fields through a reference (#266): a `&`
// parameter, `&self`, a local `&`, explicit `(*p).x`, nested fields,
// a reference stored in a struct field, fn arguments, a method
// receiver, a returned field and match / if-let scrutinees. Each read is drawn on the
// reference's column ("the resource p points to is read through
// p.x"); `&p.name` is a reborrow of the field through p.
struct Inner {
    v: i32,
}

struct P {
    x: i32,
    name: String,
    inner: Inner,
}

struct W<'a> {
    r: &'a P,
}

impl P {
    fn get_x(&self) -> i32 {
        self.x
    }
}

struct T {
    n: Option<i32>,
    label: Option<String>,
}

impl T {
    fn num(&self) -> i32 {
        match self.n {
            Some(v) => v,
            None => 0,
        }
    }

    fn label_len(&self) -> usize {
        if let Some(s) = &self.label {
            s.len()
        } else {
            0
        }
    }
}

fn take(n: i32) -> i32 {
    n
}

fn look(s: &String) -> usize {
    s.len()
}

fn show(p: &P) -> i32 {
    let a = p.x;
    let n = &p.name;
    println!("{}", n);
    let v = p.inner.v;
    let w = (*p).x;
    let t = take(p.x);
    let l = look(&p.name);
    let k = p.name.len();
    println!("{} {} {} {}", v, w, t, l + k);
    a
}

fn main() {
    let q = P { x: 1, name: String::from("a"), inner: Inner { v: 2 } };
    let r = &q;
    let b = r.x;
    let h = W { r: &q };
    let c = h.r.x;
    println!("{} {} {} {}", b, c, show(&q), q.get_x());
    let t = T { n: Some(4), label: Some(String::from("t")) };
    println!("{} {}", t.num(), t.label_len());
}

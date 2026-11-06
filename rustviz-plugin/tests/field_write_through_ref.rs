// Writes to a struct's fields through a reference (#147): a `&mut`
// parameter, `&mut self`, a local `&mut`, a reference stored in a
// struct field, explicit `(*p).x`, nested fields and tuple-struct
// fields. Each write is drawn on the reference's column ("the
// resource p points to is modified through p.x"); the reference
// never owns the field.
struct Inner {
    v: i32,
}

struct P {
    x: i32,
    name: String,
    inner: Inner,
}

struct T(i32, String);

struct W<'a> {
    r: &'a mut P,
}

struct Counter {
    c: i32,
}

impl Counter {
    fn bump(&mut self) {
        self.c = self.c + 1;
    }
}

fn update(p: &mut P, t: &mut T, a: String, z: i32) {
    p.x = 5;
    p.x = z;
    p.name = a;
    p.name += "!";
    p.inner.v += 1;
    (*p).x = 6;
    t.0 = 7;
    t.1 = String::from("t");
}

fn main() {
    let mut q = P { x: 1, name: String::from("a"), inner: Inner { v: 2 } };
    let mut t = T(0, String::new());
    update(&mut q, &mut t, String::from("b"), 3);
    let r = &mut q;
    r.x = 8;
    let w = W { r: &mut q };
    w.r.x = 9;
    let mut k = Counter { c: 0 };
    k.bump();
    println!("{} {} {}", q.x, t.0, k.c);
}

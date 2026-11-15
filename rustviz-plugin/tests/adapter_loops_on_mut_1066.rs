struct Two {
    a: Vec<i32>,
    b: Vec<i32>,
    n: i32,
}
impl Two {
    fn add(&mut self) {
        for (x, y) in self.a.iter_mut().zip(self.b.iter()) {
            *x += *y;
        }
        self.n += 1;
    }
    fn bump(&mut self) {
        for (i, e) in self.a.iter_mut().enumerate() {
            *e += i as i32;
        }
    }
}
fn add(a: &mut Vec<i32>, b: &mut Vec<i32>) {
    for (x, y) in a.iter_mut().zip(b.iter()) {
        *x += *y;
    }
    b.push(0);
}
fn both(a: &mut Vec<i32>, b: &mut Vec<i32>) {
    for (x, y) in a.iter_mut().zip(b.iter_mut()) {
        *x += 1;
        *y += 2;
    }
}
fn bump(v: &mut Vec<i32>) {
    for (i, e) in v.iter_mut().enumerate() {
        *e += i as i32;
    }
}
fn tail(v: &mut Vec<i32>) {
    for e in v.iter_mut().rev().skip(1).take(2) {
        *e += 1;
    }
}
fn main() {
    let mut t = Two { a: vec![1], b: vec![2], n: 0 };
    t.add();
    t.bump();
    let mut p = vec![1];
    let mut q = vec![2];
    add(&mut p, &mut q);
    both(&mut p, &mut q);
    bump(&mut p);
    tail(&mut p);
}

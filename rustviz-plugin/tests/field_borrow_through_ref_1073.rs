// #1073: a borrow of one field of what a `&mut` reference points to
// restricts only that field. In each function below the other field
// (`n`) is written while `items` is borrowed, and rustc accepts it;
// `self.items.len()` / `r.items.len()` there would be E0502
// (`self.items.push(0)` in `total` and `biggest`). The
// reference's own line keeps its normal style, as `s`'s does in
// `bump_local` while `s.items` is lent.
struct Bag {
    items: Vec<i32>,
    n: i32,
}
impl Bag {
    fn bump(&mut self) {
        for e in self.items.iter_mut() {
            *e += 1;
            self.n += 1;
        }
        self.items.push(0);
    }
    fn total(&mut self) -> i32 {
        let mut t = 0;
        for x in self.items.iter() {
            t += *x;
            self.n += 1;
        }
        t
    }
    fn first(&mut self) {
        let first = self.items.first_mut().unwrap();
        self.n += 1;
        *first += 1;
    }
    fn peek(&mut self) {
        if let Some(x) = self.items.last_mut() {
            self.n += 1;
            *x += 1;
        }
    }
    fn biggest(&mut self) -> i32 {
        let m = self.items.iter().max().unwrap();
        self.n += 1;
        *m
    }
}
fn bump_ref(r: &mut Bag) {
    for e in r.items.iter_mut() {
        *e += 1;
        r.n += 1;
    }
    r.items.push(0);
}
fn bump_local() {
    let mut s = Bag { items: vec![1], n: 0 };
    for e in s.items.iter_mut() {
        *e += 1;
        s.n += 1;
    }
    s.items.push(0);
}
fn main() {
    let mut b = Bag { items: vec![1], n: 0 };
    b.bump();
    b.total();
    b.first();
    b.peek();
    b.biggest();
    bump_ref(&mut b);
    bump_local();
}

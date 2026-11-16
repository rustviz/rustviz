// #1093: a closure captures just the fields of `*self` its body uses
// (edition 2021), so a value holding it borrows only those fields.
// In each function below another field (`other`) is used while the
// borrow is live, and rustc accepts it; using a captured field there
// would not compile: after `let it` in `read`, `self.items.push(0)`
// is E0502 and `self.n += 1` E0506; in `bump`, whose closure writes
// `self.n`, reading `self.n` is E0503; in the loop of `looped`,
// `self.n += 1` is E0506 and `self.items.push(0)` E0502; in `count`,
// `self.n += 1` is E0506. (A `move` closure here would move `self`
// itself into the closure, which rustc rejects while `self.items` is
// borrowed.)
struct Bag {
    items: Vec<i32>,
    other: Vec<i32>,
    n: usize,
}

impl Bag {
    fn read(&mut self) -> usize {
        let it = self.items.iter().map(|x| *x as usize + self.n);
        self.other.push(0);
        it.count()
    }

    fn bump(&mut self) -> usize {
        let it = self.items.iter().map(|x| {
            self.n += 1;
            *x as usize
        });
        self.other.push(0);
        it.count()
    }

    fn looped(&mut self) {
        for x in self.items.iter().map(|x| *x as usize + self.n) {
            self.other.push(x as i32);
        }
        self.n += 1;
    }

    fn count(&mut self) -> usize {
        let it = (0..3).map(|i| i + self.n);
        self.other.push(0);
        it.sum()
    }
}

fn main() {
    let mut b = Bag { items: vec![1, 2], other: vec![], n: 0 };
    b.read();
    b.bump();
    b.looped();
    b.count();
}
